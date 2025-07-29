use ractor::{Actor, ActorRef, ActorProcessingErr};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use crate::config::Config;
use crate::messages::{ToolMessage, ChatMessage};
use uuid::Uuid;

/// Actor for writing files
pub struct WriteActor {
    config: Config,
}

/// Write actor state
pub struct WriteState;

#[derive(Debug, Serialize, Deserialize)]
struct WriteParams {
    file_path: String,
    content: String,
}

impl Actor for WriteActor {
    type Msg = ToolMessage;
    type State = WriteState;
    type Arguments = Config;
    
    async fn pre_start(
        &self,
        _myself: ActorRef<Self::Msg>,
        _config: Self::Arguments,
    ) -> Result<Self::State, ActorProcessingErr> {
        tracing::debug!("Write actor starting");
        Ok(WriteState)
    }
    
    async fn handle(
        &self,
        _myself: ActorRef<Self::Msg>,
        msg: Self::Msg,
        _state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        match msg {
            ToolMessage::Execute { id, params, chat_ref } => {
                tracing::info!("Executing write tool with params: {:?}", params);
                
                // Parse parameters
                let write_params: WriteParams = match serde_json::from_value(params) {
                    Ok(p) => p,
                    Err(e) => {
                        chat_ref.send_message(ChatMessage::ToolResult {
                            id,
                            result: format!("Error: Invalid parameters - {}", e),
                        })?;
                        return Ok(());
                    }
                };
                
                // Validate path is absolute
                let path = Path::new(&write_params.file_path);
                if !path.is_absolute() {
                    chat_ref.send_message(ChatMessage::ToolResult {
                        id,
                        result: format!("Error: File path must be absolute, but was relative: {}", write_params.file_path),
                    })?;
                    return Ok(());
                }
                
                // Execute write operation
                let result = match self.write_file(&write_params) {
                    Ok(info) => info,
                    Err(e) => format!("Error: {}", e),
                };
                
                // Send result back to chat
                chat_ref.send_message(ChatMessage::ToolResult {
                    id,
                    result,
                })?;
            }
            
            ToolMessage::Cancel { id } => {
                tracing::debug!("Cancelling write operation {}", id);
                // Write operations are synchronous, nothing to cancel
            }
            
            ToolMessage::StreamUpdate { .. } => {
                // Write doesn't stream updates
            }
        }
        
        Ok(())
    }
}

impl WriteActor {
    pub fn new(config: Config) -> Self {
        Self { config }
    }
    
    fn write_file(&self, params: &WriteParams) -> Result<String, String> {
        let path = Path::new(&params.file_path);
        
        // Check if path exists and is a directory
        if path.exists() {
            let metadata = fs::metadata(&path)
                .map_err(|e| format!("Cannot access path '{}': {}", params.file_path, e))?;
            
            if metadata.is_dir() {
                return Err(format!("Path is a directory, not a file: {}", params.file_path));
            }
        }
        
        // Create parent directories if they don't exist
        if let Some(parent) = path.parent() {
            if !parent.exists() {
                fs::create_dir_all(parent)
                    .map_err(|e| format!("Cannot create parent directories for '{}': {}", params.file_path, e))?;
            }
        }
        
        // Determine if this is a new file or overwrite
        let is_new_file = !path.exists();
        let prev_size = if !is_new_file {
            fs::metadata(&path).ok().map(|m| m.len()).unwrap_or(0)
        } else {
            0
        };
        
        // Write the file
        fs::write(&path, &params.content)
            .map_err(|e| format!("Cannot write to file '{}': {}", params.file_path, e))?;
        
        // Get file info after write
        let metadata = fs::metadata(&path)
            .map_err(|e| format!("Cannot read metadata after write: {}", e))?;
        let new_size = metadata.len();
        let lines = params.content.lines().count();
        
        // Format success message
        if is_new_file {
            Ok(format!(
                "Successfully created new file: {}\n\nFile details:\n- Size: {} bytes\n- Lines: {}\n- Path: {}",
                params.file_path,
                new_size,
                lines,
                params.file_path
            ))
        } else {
            Ok(format!(
                "Successfully overwrote file: {}\n\nFile details:\n- Previous size: {} bytes\n- New size: {} bytes\n- Lines: {}\n- Path: {}",
                params.file_path,
                prev_size,
                new_size,
                lines,
                params.file_path
            ))
        }
    }
}