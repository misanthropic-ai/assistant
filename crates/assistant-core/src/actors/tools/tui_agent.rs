use ractor::{Actor, ActorRef, ActorProcessingErr};
use serde::{Deserialize, Serialize};
use serde_json::json;
use crate::config::Config;
use crate::messages::{ToolMessage, ChatMessage};
use anyhow::Result;

/// TUI agent tool - high-level interface for TUI interaction
pub struct TuiAgentActor {
    #[allow(dead_code)]
    config: Config,
}

pub struct TuiAgentState;

/// TUI agent action types (simplified interface)
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum TuiAgentAction {
    /// Start interacting with a TUI application
    StartApp {
        command: String,
        #[serde(default)]
        task: Option<String>,
    },
    
    /// Navigate to something in the TUI
    NavigateTo {
        target: String,
    },
    
    /// Perform a task in the current TUI
    PerformTask {
        task: String,
    },
    
    /// Analyze what's currently on screen
    AnalyzeScreen,
    
    /// Execute a sequence of actions
    ExecuteSteps {
        steps: Vec<String>,
    },
    
    /// Exit the current TUI application
    ExitApp,
}

impl TuiAgentActor {
    pub async fn new(config: Config) -> Result<Self> {
        Ok(Self {
            config,
        })
    }
}

impl Actor for TuiAgentActor {
    type Msg = ToolMessage;
    type State = TuiAgentState;
    type Arguments = Config;
    
    async fn pre_start(
        &self,
        _myself: ActorRef<Self::Msg>,
        _config: Self::Arguments,
    ) -> Result<Self::State, ActorProcessingErr> {
        Ok(TuiAgentState)
    }
    
    async fn handle(
        &self,
        _myself: ActorRef<Self::Msg>,
        msg: Self::Msg,
        _state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        match msg {
            ToolMessage::Execute { id, params, chat_ref } => {
                let action: TuiAgentAction = match serde_json::from_value(params) {
                    Ok(a) => a,
                    Err(e) => {
                        chat_ref.send_message(ChatMessage::ToolResult {
                            id,
                            result: format!("Error parsing TUI agent parameters: {}", e),
                        })?;
                        return Ok(());
                    }
                };
                
                let result = match action {
                    TuiAgentAction::StartApp { command, task } => {
                        // This is a simplified interface - the actual work would be delegated
                        let message = if let Some(task_desc) = task {
                            format!(
                                "To start '{}' and {}, use the tui_control tool to:\n\
                                1. Start a session with the command\n\
                                2. Capture the initial screen\n\
                                3. Then perform the requested task",
                                command, task_desc
                            )
                        } else {
                            format!(
                                "To start '{}', use the tui_control tool to:\n\
                                1. Start a session with the command\n\
                                2. Capture the initial screen\n\
                                3. Begin interacting as needed",
                                command
                            )
                        };
                        
                        json!({
                            "success": true,
                            "message": message,
                            "next_steps": ["start_session", "capture_screen", "analyze"]
                        }).to_string()
                    }
                    
                    TuiAgentAction::NavigateTo { target } => {
                        json!({
                            "success": true,
                            "message": format!("To navigate to '{}', analyze the current screen and send appropriate keyboard commands", target),
                            "hint": "Common navigation: arrow keys, tab, '/', ctrl+f for search"
                        }).to_string()
                    }
                    
                    TuiAgentAction::PerformTask { task } => {
                        json!({
                            "success": true,
                            "message": format!("To perform '{}', break it down into steps and execute them", task),
                            "approach": "Capture screen -> Analyze -> Send keys -> Verify result"
                        }).to_string()
                    }
                    
                    TuiAgentAction::AnalyzeScreen => {
                        json!({
                            "success": true,
                            "message": "Use tui_control to capture the screen, then analyze the content",
                            "tips": ["Look for menus", "Check status lines", "Identify active areas"]
                        }).to_string()
                    }
                    
                    TuiAgentAction::ExecuteSteps { steps } => {
                        json!({
                            "success": true,
                            "message": "Execute each step in sequence",
                            "steps": steps,
                            "reminder": "Capture screen between steps to verify progress"
                        }).to_string()
                    }
                    
                    TuiAgentAction::ExitApp => {
                        json!({
                            "success": true,
                            "message": "To exit, send the appropriate quit command (e.g., ':q', 'ctrl+c', 'q')",
                            "then": "End the tui_control session"
                        }).to_string()
                    }
                };
                
                chat_ref.send_message(ChatMessage::ToolResult { id, result })?;
            }
            
            ToolMessage::Cancel { .. } => {
                // TUI agent operations are guided, not long-running
            }
            
            ToolMessage::StreamUpdate { .. } => {
                // No streaming for TUI agent
            }
        }
        
        Ok(())
    }
}