use ractor::{Actor, ActorRef, ActorProcessingErr};
use serde::{Deserialize, Serialize};
use serde_json::json;
use crate::config::Config;
use crate::messages::{ToolMessage, ChatMessage};
use crate::persistence::{Database, TuiSessionManager};
use anyhow::Result;
use std::process::Command;
use std::collections::HashMap;
use uuid::Uuid;

/// Actor for controlling TUI applications through tmux
pub struct TuiControlActor {
    #[allow(dead_code)]
    config: Config,
    session_manager: Option<TuiSessionManager>,
}

/// State tracking active TUI sessions
pub struct TuiControlState {
    /// Map of session IDs to tmux session names
    sessions: HashMap<Uuid, TmuxSession>,
    /// Current chat session ID (if available)
    chat_session_id: Option<String>,
}

#[derive(Debug, Clone)]
struct TmuxSession {
    name: String,
    command: String,
    created_at: std::time::Instant,
}

/// TUI control action types
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum TuiAction {
    /// Start a new TUI session
    StartSession {
        command: String,
        #[serde(default)]
        width: Option<u32>,
        #[serde(default)]
        height: Option<u32>,
    },
    
    /// Capture the current screen content
    CaptureScreen {
        #[serde(default)]
        session_id: Option<Uuid>,
        #[serde(default)]
        include_ansi: bool,
    },
    
    /// Send keyboard input to the TUI
    SendKeys {
        keys: String,
        #[serde(default)]
        session_id: Option<Uuid>,
    },
    
    /// Send text (types it character by character)
    SendText {
        text: String,
        #[serde(default)]
        session_id: Option<Uuid>,
    },
    
    /// End a TUI session
    EndSession {
        #[serde(default)]
        session_id: Option<Uuid>,
    },
    
    /// List active sessions
    ListSessions,
    
    /// Get screen dimensions
    GetDimensions {
        #[serde(default)]
        session_id: Option<Uuid>,
    },
}

/// Screen capture response
#[derive(Debug, Serialize, Deserialize)]
pub struct TuiScreen {
    /// Raw text content of the screen
    pub content: String,
    /// Width in columns
    pub width: usize,
    /// Height in rows
    pub height: usize,
    /// 2D grid representation (if requested)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub grid: Option<Vec<Vec<char>>>,
}

impl TuiControlActor {
    pub fn new(config: Config) -> Self {
        Self { 
            config,
            session_manager: None,
        }
    }
    
    /// Create with database persistence
    pub async fn with_persistence(config: Config) -> Result<Self> {
        // Get database path from config
        let db_path = config.session.database_path.clone()
            .unwrap_or_else(|| {
                dirs::home_dir()
                    .unwrap_or_else(|| std::path::PathBuf::from("/tmp"))
                    .join(".assistant")
                    .join("assistant.db")
            });
        
        let database = Database::new(&db_path).await?;
        let session_manager = TuiSessionManager::new(database);
        
        Ok(Self { 
            config,
            session_manager: Some(session_manager),
        })
    }
    
    /// Generate a unique tmux session name
    fn generate_session_name() -> String {
        format!("tui_{}", Uuid::new_v4().simple())
    }
    
    /// Start a new tmux session
    async fn start_session(&self, command: &str, width: Option<u32>, height: Option<u32>) -> Result<(Uuid, String)> {
        let session_name = Self::generate_session_name();
        let session_id = Uuid::new_v4();
        
        // Build tmux command
        let mut tmux_cmd = Command::new("tmux");
        tmux_cmd.arg("new-session")
            .arg("-d")  // Detached
            .arg("-s").arg(&session_name);
        
        // Set dimensions if provided
        if let Some(w) = width {
            tmux_cmd.arg("-x").arg(w.to_string());
        }
        if let Some(h) = height {
            tmux_cmd.arg("-y").arg(h.to_string());
        }
        
        // Add the command to run
        tmux_cmd.arg(command);
        
        // Execute
        let output = tmux_cmd.output()?;
        
        if !output.status.success() {
            return Err(anyhow::anyhow!(
                "Failed to start tmux session: {}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }
        
        Ok((session_id, session_name))
    }
    
    /// Capture screen content from tmux
    async fn capture_screen(&self, session_name: &str, include_ansi: bool) -> Result<TuiScreen> {
        let mut cmd = Command::new("tmux");
        cmd.arg("capture-pane")
            .arg("-t").arg(session_name)
            .arg("-p");  // Print to stdout
        
        if include_ansi {
            cmd.arg("-e");  // Include escape sequences
        }
        
        let output = cmd.output()?;
        
        if !output.status.success() {
            return Err(anyhow::anyhow!(
                "Failed to capture screen: {}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }
        
        let content = String::from_utf8_lossy(&output.stdout).to_string();
        
        // Get dimensions
        let (width, height) = self.get_dimensions(session_name).await?;
        
        // Parse into grid if not including ANSI
        let grid = if !include_ansi {
            Some(self.parse_to_grid(&content, width, height))
        } else {
            None
        };
        
        Ok(TuiScreen {
            content,
            width,
            height,
            grid,
        })
    }
    
    /// Get tmux pane dimensions
    async fn get_dimensions(&self, session_name: &str) -> Result<(usize, usize)> {
        // Get width
        let width_output = Command::new("tmux")
            .arg("display-message")
            .arg("-t").arg(session_name)
            .arg("-p").arg("#{pane_width}")
            .output()?;
        
        // Get height
        let height_output = Command::new("tmux")
            .arg("display-message")
            .arg("-t").arg(session_name)
            .arg("-p").arg("#{pane_height}")
            .output()?;
        
        if !width_output.status.success() || !height_output.status.success() {
            return Err(anyhow::anyhow!("Failed to get pane dimensions"));
        }
        
        let width = String::from_utf8_lossy(&width_output.stdout)
            .trim()
            .parse::<usize>()?;
        let height = String::from_utf8_lossy(&height_output.stdout)
            .trim()
            .parse::<usize>()?;
        
        Ok((width, height))
    }
    
    /// Parse screen content into a 2D grid
    fn parse_to_grid(&self, content: &str, width: usize, height: usize) -> Vec<Vec<char>> {
        let lines: Vec<&str> = content.lines().collect();
        let mut grid = vec![vec![' '; width]; height];
        
        for (y, line) in lines.iter().enumerate() {
            if y >= height {
                break;
            }
            for (x, ch) in line.chars().enumerate() {
                if x >= width {
                    break;
                }
                grid[y][x] = ch;
            }
        }
        
        grid
    }
    
    /// Send keys to tmux session
    async fn send_keys(&self, session_name: &str, keys: &str) -> Result<()> {
        let output = Command::new("tmux")
            .arg("send-keys")
            .arg("-t").arg(session_name)
            .arg(keys)
            .output()?;
        
        if !output.status.success() {
            return Err(anyhow::anyhow!(
                "Failed to send keys: {}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }
        
        Ok(())
    }
    
    /// Send text by typing each character
    async fn send_text(&self, session_name: &str, text: &str) -> Result<()> {
        // Escape special characters for tmux
        let escaped = text
            .replace("\\", "\\\\")
            .replace("\"", "\\\"")
            .replace("$", "\\$")
            .replace("`", "\\`");
        
        let output = Command::new("tmux")
            .arg("send-keys")
            .arg("-t").arg(session_name)
            .arg("-l")  // Literal keys (disable key name lookup)
            .arg(&escaped)
            .output()?;
        
        if !output.status.success() {
            return Err(anyhow::anyhow!(
                "Failed to send text: {}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }
        
        Ok(())
    }
    
    /// End a tmux session
    async fn end_session(&self, session_name: &str) -> Result<()> {
        let output = Command::new("tmux")
            .arg("kill-session")
            .arg("-t").arg(session_name)
            .output()?;
        
        if !output.status.success() {
            // Session might already be gone
            let stderr = String::from_utf8_lossy(&output.stderr);
            if !stderr.contains("can't find session") {
                return Err(anyhow::anyhow!("Failed to end session: {}", stderr));
            }
        }
        
        Ok(())
    }
}

impl Actor for TuiControlActor {
    type Msg = ToolMessage;
    type State = TuiControlState;
    type Arguments = Config;
    
    async fn pre_start(
        &self,
        _myself: ActorRef<Self::Msg>,
        _config: Self::Arguments,
    ) -> Result<Self::State, ActorProcessingErr> {
        // Verify existing sessions if persistence is enabled
        if let Some(ref manager) = self.session_manager {
            if let Err(e) = manager.verify_sessions().await {
                tracing::warn!("Failed to verify TUI sessions: {}", e);
            }
            
            // Load active sessions from database
            match manager.list_active_sessions().await {
                Ok(db_sessions) => {
                    let mut sessions = HashMap::new();
                    for db_session in db_sessions {
                        if let Ok(uuid) = Uuid::parse_str(&db_session.id) {
                            sessions.insert(uuid, TmuxSession {
                                name: db_session.tmux_session_name,
                                command: db_session.command,
                                created_at: std::time::Instant::now(),
                            });
                        }
                    }
                    return Ok(TuiControlState {
                        sessions,
                        chat_session_id: None,
                    });
                }
                Err(e) => {
                    tracing::warn!("Failed to load TUI sessions from database: {}", e);
                }
            }
        }
        
        Ok(TuiControlState {
            sessions: HashMap::new(),
            chat_session_id: None,
        })
    }
    
    async fn handle(
        &self,
        _myself: ActorRef<Self::Msg>,
        msg: Self::Msg,
        state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        match msg {
            ToolMessage::Execute { id, params, chat_ref } => {
                let action: TuiAction = match serde_json::from_value(params) {
                    Ok(a) => a,
                    Err(e) => {
                        chat_ref.send_message(ChatMessage::ToolResult {
                            id,
                            result: format!("Error parsing TUI control parameters: {}", e),
                        })?;
                        return Ok(());
                    }
                };
                
                let result = match action {
                    TuiAction::StartSession { command, width, height } => {
                        match self.start_session(&command, width, height).await {
                            Ok((session_id, session_name)) => {
                                state.sessions.insert(session_id, TmuxSession {
                                    name: session_name.clone(),
                                    command: command.clone(),
                                    created_at: std::time::Instant::now(),
                                });
                                
                                // Persist to database if available
                                if let Some(ref manager) = self.session_manager {
                                    if let Err(e) = manager.create_session(
                                        state.chat_session_id.as_deref(),
                                        &session_name,
                                        &command,
                                    ).await {
                                        tracing::warn!("Failed to persist TUI session: {}", e);
                                    }
                                }
                                
                                json!({
                                    "success": true,
                                    "session_id": session_id,
                                    "message": format!("Started TUI session for: {}", command)
                                }).to_string()
                            }
                            Err(e) => json!({
                                "success": false,
                                "error": e.to_string()
                            }).to_string()
                        }
                    }
                    
                    TuiAction::CaptureScreen { session_id, include_ansi } => {
                        let session = if let Some(id) = session_id {
                            state.sessions.get(&id)
                        } else {
                            state.sessions.values().next()
                        };
                        
                        match session {
                            Some(session) => {
                                // Update last accessed time in database
                                if let Some(ref manager) = self.session_manager {
                                    if let Some(db_session) = manager.get_session_by_tmux_name(&session.name).await.ok().flatten() {
                                        let _ = manager.update_last_accessed(&db_session.id).await;
                                    }
                                }
                                
                                match self.capture_screen(&session.name, include_ansi).await {
                                    Ok(screen) => {
                                        serde_json::to_string(&json!({
                                            "success": true,
                                            "screen": screen
                                        })).unwrap_or_else(|e| json!({
                                            "success": false,
                                            "error": format!("Failed to serialize screen: {}", e)
                                        }).to_string())
                                    }
                                    Err(e) => json!({
                                        "success": false,
                                        "error": e.to_string()
                                    }).to_string()
                                }
                            }
                            None => json!({
                                "success": false,
                                "error": "No active TUI session found"
                            }).to_string()
                        }
                    }
                    
                    TuiAction::SendKeys { keys, session_id } => {
                        let session = if let Some(id) = session_id {
                            state.sessions.get(&id)
                        } else {
                            state.sessions.values().next()
                        };
                        
                        match session {
                            Some(session) => {
                                match self.send_keys(&session.name, &keys).await {
                                    Ok(()) => json!({
                                        "success": true,
                                        "message": format!("Sent keys: {}", keys)
                                    }).to_string(),
                                    Err(e) => json!({
                                        "success": false,
                                        "error": e.to_string()
                                    }).to_string()
                                }
                            }
                            None => json!({
                                "success": false,
                                "error": "No active TUI session found"
                            }).to_string()
                        }
                    }
                    
                    TuiAction::SendText { text, session_id } => {
                        let session = if let Some(id) = session_id {
                            state.sessions.get(&id)
                        } else {
                            state.sessions.values().next()
                        };
                        
                        match session {
                            Some(session) => {
                                match self.send_text(&session.name, &text).await {
                                    Ok(()) => json!({
                                        "success": true,
                                        "message": format!("Typed text: {}", text)
                                    }).to_string(),
                                    Err(e) => json!({
                                        "success": false,
                                        "error": e.to_string()
                                    }).to_string()
                                }
                            }
                            None => json!({
                                "success": false,
                                "error": "No active TUI session found"
                            }).to_string()
                        }
                    }
                    
                    TuiAction::EndSession { session_id } => {
                        let session = if let Some(id) = session_id {
                            state.sessions.remove(&id)
                        } else {
                            // Remove the first session if no ID specified
                            if let Some(id) = state.sessions.keys().next().cloned() {
                                state.sessions.remove(&id)
                            } else {
                                None
                            }
                        };
                        
                        match session {
                            Some(session) => {
                                // Update status in database
                                if let Some(ref manager) = self.session_manager {
                                    if let Some(db_session) = manager.get_session_by_tmux_name(&session.name).await.ok().flatten() {
                                        let _ = manager.update_status(&db_session.id, "terminated").await;
                                    }
                                }
                                
                                match self.end_session(&session.name).await {
                                    Ok(()) => json!({
                                        "success": true,
                                        "message": "TUI session ended"
                                    }).to_string(),
                                    Err(e) => json!({
                                        "success": false,
                                        "error": e.to_string()
                                    }).to_string()
                                }
                            }
                            None => json!({
                                "success": false,
                                "error": "No active TUI session found"
                            }).to_string()
                        }
                    }
                    
                    TuiAction::ListSessions => {
                        let sessions: Vec<_> = state.sessions.iter().map(|(id, session)| {
                            json!({
                                "session_id": id,
                                "command": session.command,
                                "duration_seconds": session.created_at.elapsed().as_secs()
                            })
                        }).collect();
                        
                        json!({
                            "success": true,
                            "sessions": sessions
                        }).to_string()
                    }
                    
                    TuiAction::GetDimensions { session_id } => {
                        let session = if let Some(id) = session_id {
                            state.sessions.get(&id)
                        } else {
                            state.sessions.values().next()
                        };
                        
                        match session {
                            Some(session) => {
                                match self.get_dimensions(&session.name).await {
                                    Ok((width, height)) => json!({
                                        "success": true,
                                        "width": width,
                                        "height": height
                                    }).to_string(),
                                    Err(e) => json!({
                                        "success": false,
                                        "error": e.to_string()
                                    }).to_string()
                                }
                            }
                            None => json!({
                                "success": false,
                                "error": "No active TUI session found"
                            }).to_string()
                        }
                    }
                };
                
                chat_ref.send_message(ChatMessage::ToolResult { id, result })?;
            }
            
            ToolMessage::Cancel { .. } => {
                // TUI control actions are synchronous
            }
            
            ToolMessage::StreamUpdate { .. } => {
                // No streaming for TUI control
            }
        }
        
        Ok(())
    }
    
    async fn post_stop(
        &self,
        _myself: ActorRef<Self::Msg>,
        state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        // Clean up all active sessions
        for session in state.sessions.values() {
            let _ = self.end_session(&session.name).await;
        }
        Ok(())
    }
}