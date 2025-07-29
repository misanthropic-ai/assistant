use ractor::{Actor, ActorRef, ActorProcessingErr};
use serde::{Deserialize, Serialize};
use std::process::Stdio;
use std::time::Duration;
use tokio::process::Command;
use tokio::time::timeout;
use tokio::io::BufReader;
use crate::config::Config;
use crate::messages::{ToolMessage, ChatMessage};

/// Actor for executing bash commands
pub struct BashActor {
    config: Config,
}

/// Bash actor state
pub struct BashState {
    working_directory: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct BashParams {
    command: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default = "default_timeout")]
    timeout: u64,
}

fn default_timeout() -> u64 {
    120000 // 2 minutes in milliseconds
}

impl Actor for BashActor {
    type Msg = ToolMessage;
    type State = BashState;
    type Arguments = Config;
    
    async fn pre_start(
        &self,
        _myself: ActorRef<Self::Msg>,
        _config: Self::Arguments,
    ) -> Result<Self::State, ActorProcessingErr> {
        tracing::debug!("Bash actor starting");
        
        // Get current working directory
        let working_directory = std::env::current_dir()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| String::from("/"));
            
        Ok(BashState { working_directory })
    }
    
    async fn handle(
        &self,
        myself: ActorRef<Self::Msg>,
        msg: Self::Msg,
        state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        match msg {
            ToolMessage::Execute { id, params, chat_ref } => {
                tracing::info!("Executing bash command with params: {:?}", params);
                
                // Parse parameters
                let bash_params: BashParams = match serde_json::from_value(params) {
                    Ok(p) => p,
                    Err(e) => {
                        chat_ref.send_message(ChatMessage::ToolResult {
                            id,
                            result: format!("Error: Invalid parameters - {}", e),
                        })?;
                        return Ok(());
                    }
                };
                
                // Validate timeout
                if bash_params.timeout > 600000 {
                    chat_ref.send_message(ChatMessage::ToolResult {
                        id,
                        result: format!("Error: Timeout exceeds maximum of 10 minutes (600000ms)"),
                    })?;
                    return Ok(());
                }
                
                // Execute command
                let result = self.execute_command(&bash_params, state, myself.clone(), id).await;
                
                // Send result back to chat
                chat_ref.send_message(ChatMessage::ToolResult {
                    id,
                    result,
                })?;
            }
            
            ToolMessage::Cancel { id } => {
                tracing::debug!("Cancelling bash operation {}", id);
                // TODO: Implement command cancellation
            }
            
            ToolMessage::StreamUpdate { id, output } => {
                tracing::debug!("Bash stream update for {}: {}", id, output);
                // This is used internally for streaming output
            }
        }
        
        Ok(())
    }
}

impl BashActor {
    pub fn new(config: Config) -> Self {
        Self { config }
    }
    
    async fn execute_command(
        &self,
        params: &BashParams,
        state: &mut BashState,
        myself: ActorRef<ToolMessage>,
        execution_id: uuid::Uuid,
    ) -> String {
        // Update working directory if cd command
        if let Some(new_dir) = self.extract_cd_path(&params.command) {
            match self.change_directory(&new_dir, &state.working_directory) {
                Ok(absolute_path) => {
                    state.working_directory = absolute_path.clone();
                    return format!("Changed directory to: {}", absolute_path);
                }
                Err(e) => {
                    return format!("Error changing directory: {}", e);
                }
            }
        }
        
        // Prepare command with shell
        let mut cmd = Command::new("bash");
        cmd.arg("-c")
            .arg(&params.command)
            .current_dir(&state.working_directory)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        
        // Add environment variables
        cmd.env("NO_COLOR", "1");
        cmd.env("TERM", "dumb");
        
        // Execute command with timeout
        let timeout_duration = Duration::from_millis(params.timeout);
        
        match timeout(timeout_duration, self.run_command(cmd, myself, execution_id)).await {
            Ok(Ok(output)) => output,
            Ok(Err(e)) => format!("Error executing command: {}", e),
            Err(_) => format!("Command timed out after {}ms", params.timeout),
        }
    }
    
    async fn run_command(
        &self,
        mut cmd: Command,
        _myself: ActorRef<ToolMessage>,
        _execution_id: uuid::Uuid,
    ) -> Result<String, String> {
        let mut child = cmd.spawn().map_err(|e| format!("Failed to spawn command: {}", e))?;
        
        let stdout = child.stdout.take().ok_or("Failed to capture stdout")?;
        let stderr = child.stderr.take().ok_or("Failed to capture stderr")?;
        
        let stdout_reader = BufReader::new(stdout);
        let stderr_reader = BufReader::new(stderr);
        
        let mut output = String::new();
        
        // Read output from both stdout and stderr
        use tokio::io::AsyncReadExt;
        
        let mut stdout_buf = Vec::new();
        let mut stderr_buf = Vec::new();
        
        // Read all stdout
        let mut stdout_reader = stdout_reader.into_inner();
        stdout_reader.read_to_end(&mut stdout_buf).await.map_err(|e| format!("Failed to read stdout: {}", e))?;
        
        // Read all stderr
        let mut stderr_reader = stderr_reader.into_inner();
        stderr_reader.read_to_end(&mut stderr_buf).await.map_err(|e| format!("Failed to read stderr: {}", e))?;
        
        // Convert to strings
        let stdout_str = String::from_utf8_lossy(&stdout_buf);
        let stderr_str = String::from_utf8_lossy(&stderr_buf);
        
        // Combine output
        output.push_str(&stdout_str);
        if !stdout_str.is_empty() && !stderr_str.is_empty() {
            output.push('\n');
        }
        output.push_str(&stderr_str);
        
        // Wait for process to complete
        let status = child.wait().await.map_err(|e| format!("Failed to wait for command: {}", e))?;
        
        if !status.success() {
            if let Some(code) = status.code() {
                output.push_str(&format!("\n\nCommand exited with code: {}", code));
            } else {
                output.push_str("\n\nCommand terminated by signal");
            }
        }
        
        // Truncate output if too large
        if output.len() > 30000 {
            output.truncate(30000);
            output.push_str("\n\n... (output truncated)");
        }
        
        Ok(output)
    }
    
    fn extract_cd_path(&self, command: &str) -> Option<String> {
        let trimmed = command.trim();
        if trimmed == "cd" {
            return Some(String::from("~"));
        }
        
        if trimmed.starts_with("cd ") {
            let path = trimmed[3..].trim();
            if !path.is_empty() && !path.contains(';') && !path.contains("&&") {
                return Some(path.to_string());
            }
        }
        
        None
    }
    
    fn change_directory(&self, path: &str, current_dir: &str) -> Result<String, String> {
        use std::path::PathBuf;
        
        let target_path = if path.starts_with('~') {
            // Handle home directory
            let home = dirs::home_dir().ok_or("Cannot determine home directory")?;
            if path == "~" {
                home
            } else {
                home.join(&path[2..])
            }
        } else if path.starts_with('/') {
            // Absolute path
            PathBuf::from(path)
        } else {
            // Relative path
            PathBuf::from(current_dir).join(path)
        };
        
        // Canonicalize the path
        let absolute_path = target_path
            .canonicalize()
            .map_err(|e| format!("Cannot resolve path '{}': {}", path, e))?;
        
        // Verify it's a directory
        if !absolute_path.is_dir() {
            return Err(format!("'{}' is not a directory", absolute_path.display()));
        }
        
        Ok(absolute_path.to_string_lossy().to_string())
    }
}