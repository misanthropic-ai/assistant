use ractor::{Actor, ActorRef, ActorProcessingErr};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use crate::config::Config;
use crate::messages::{ToolMessage, ChatMessage};
use uuid::Uuid;

/// Actor for reading files
pub struct ReadActor {
    config: Config,
}

/// Read actor state
pub struct ReadState;

#[derive(Debug, Serialize, Deserialize)]
struct ReadParams {
    path: String,
    #[serde(default)]
    offset: Option<usize>,
    #[serde(default)]
    limit: Option<usize>,
}

impl Actor for ReadActor {
    type Msg = ToolMessage;
    type State = ReadState;
    type Arguments = Config;
    
    async fn pre_start(
        &self,
        _myself: ActorRef<Self::Msg>,
        _config: Self::Arguments,
    ) -> Result<Self::State, ActorProcessingErr> {
        tracing::debug!("Read actor starting");
        Ok(ReadState)
    }
    
    async fn handle(
        &self,
        _myself: ActorRef<Self::Msg>,
        msg: Self::Msg,
        _state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        match msg {
            ToolMessage::Execute { id, params, chat_ref } => {
                tracing::info!("Executing read tool with params: {:?}", params);
                
                // Parse parameters
                let read_params: ReadParams = match serde_json::from_value(params) {
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
                let path = Path::new(&read_params.path);
                if !path.is_absolute() {
                    chat_ref.send_message(ChatMessage::ToolResult {
                        id,
                        result: format!("Error: File path must be absolute, but was relative: {}", read_params.path),
                    })?;
                    return Ok(());
                }
                
                // Execute read operation
                let result = match fs::read_to_string(&read_params.path) {
                    Ok(content) => {
                        let mut output = content;
                        
                        // Apply offset and limit if specified
                        if let Some(offset) = read_params.offset {
                            let lines: Vec<&str> = output.lines().collect();
                            let remaining_lines = lines.len().saturating_sub(offset);
                            let limit = read_params.limit.unwrap_or(remaining_lines);
                            output = lines.iter()
                                .skip(offset)
                                .take(limit)
                                .enumerate()
                                .map(|(i, line)| format!("{:>5}│{}", offset + i + 1, line))
                                .collect::<Vec<_>>()
                                .join("\n");
                        }
                        
                        output
                    }
                    Err(e) => {
                        format!("Error reading file '{}': {}", read_params.path, e)
                    }
                };
                
                // Send result back to chat
                chat_ref.send_message(ChatMessage::ToolResult {
                    id,
                    result,
                })?;
            }
            
            ToolMessage::Cancel { id } => {
                tracing::debug!("Cancelling read operation {}", id);
                // Read operations are synchronous, nothing to cancel
            }
            
            ToolMessage::StreamUpdate { .. } => {
                // Read doesn't stream updates
            }
        }
        
        Ok(())
    }
}

impl ReadActor {
    pub fn new(config: Config) -> Self {
        Self {
            config,
        }
    }
}