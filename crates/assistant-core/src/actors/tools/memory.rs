use ractor::{Actor, ActorRef, ActorProcessingErr};
use std::collections::HashMap;
use serde::{Deserialize, Serialize};
use crate::config::Config;
use crate::messages::{ToolMessage, ChatMessage};
use uuid::Uuid;

/// Actor for memory/context management
pub struct MemoryActor {
    config: Config,
}

/// Memory state
pub struct MemoryState {
    /// Stored memories by key
    memories: HashMap<String, String>,
    /// Session context
    context: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "operation")]
pub enum MemoryOperation {
    Save { key: String, content: String },
    Load { key: String },
    List,
    Clear { key: Option<String> },
}

impl Actor for MemoryActor {
    type Msg = ToolMessage;
    type State = MemoryState;
    type Arguments = Config;
    
    async fn pre_start(
        &self,
        _myself: ActorRef<Self::Msg>,
        _config: Self::Arguments,
    ) -> Result<Self::State, ActorProcessingErr> {
        tracing::debug!("Memory actor starting");
        Ok(MemoryState {
            memories: HashMap::new(),
            context: Vec::new(),
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
                tracing::info!("Memory tool execution with params: {:?}", params);
                
                // Parse operation
                let operation: MemoryOperation = match serde_json::from_value(params) {
                    Ok(op) => op,
                    Err(e) => {
                        chat_ref.send_message(ChatMessage::ToolResult {
                            id,
                            result: format!("Error: Invalid parameters - {}", e),
                        })?;
                        return Ok(());
                    }
                };
                
                let result = match operation {
                    MemoryOperation::Save { key, content } => {
                        state.memories.insert(key.clone(), content.clone());
                        format!("Saved {} bytes to memory key '{}'", content.len(), key)
                    }
                    
                    MemoryOperation::Load { key } => {
                        match state.memories.get(&key) {
                            Some(content) => content.clone(),
                            None => format!("Memory key '{}' not found", key),
                        }
                    }
                    
                    MemoryOperation::List => {
                        let keys: Vec<&String> = state.memories.keys().collect();
                        if keys.is_empty() {
                            "No memory keys found".to_string()
                        } else {
                            format!("Memory keys: {:?}", keys)
                        }
                    }
                    
                    MemoryOperation::Clear { key } => {
                        match key {
                            Some(k) => {
                                state.memories.remove(&k);
                                format!("Cleared memory key: {}", k)
                            }
                            None => {
                                state.memories.clear();
                                "Cleared all memory".to_string()
                            }
                        }
                    }
                };
                
                // Send result back to chat actor
                chat_ref.send_message(ChatMessage::ToolResult {
                    id,
                    result,
                })?;
            }
            
            ToolMessage::Cancel { id } => {
                tracing::debug!("Cancelling memory operation {}", id);
                // Memory operations are synchronous, nothing to cancel
            }
            
            ToolMessage::StreamUpdate { .. } => {
                // Memory doesn't stream updates
            }
        }
        
        Ok(())
    }
}

impl MemoryActor {
    pub fn new(config: Config) -> Self {
        Self { config }
    }
}