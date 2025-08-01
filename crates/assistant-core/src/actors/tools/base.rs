use ractor::{Actor, ActorRef, ActorProcessingErr};
use crate::config::Config;
use crate::messages::{ToolMessage, ChatMessage};

/// Base implementation for tool actors with common functionality
/// 
/// This can be used as a reference implementation or extended
/// for tools that need more complex behavior.
pub struct BaseToolActor {
    name: String,
    #[allow(dead_code)]    
    config: Config,
}

pub struct BaseToolState;

impl Actor for BaseToolActor {
    type Msg = ToolMessage;
    type State = BaseToolState;
    type Arguments = Config;
    
    async fn pre_start(
        &self,
        _myself: ActorRef<Self::Msg>,
        _config: Self::Arguments,
    ) -> Result<Self::State, ActorProcessingErr> {
        tracing::debug!("Tool actor '{}' starting", self.name);
        Ok(BaseToolState)
    }
    
    async fn handle(
        &self,
        _myself: ActorRef<Self::Msg>,
        msg: Self::Msg,
        _state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        match msg {
            ToolMessage::Execute { id, params, chat_ref } => {
                tracing::info!("Tool '{}' executing with params: {:?}", self.name, params);
                
                // This is a base implementation - specific tools should override
                let result = format!("Tool '{}' executed with params: {}", self.name, params);
                
                // Send result back to chat
                chat_ref.send_message(ChatMessage::ToolResult {
                    id,
                    result,
                })?;
            }
            
            ToolMessage::Cancel { id } => {
                tracing::debug!("Cancelling tool '{}' execution: {}", self.name, id);
                // Most tools will be synchronous, but async tools can override this
            }
            
            ToolMessage::StreamUpdate { id, output } => {
                tracing::debug!("Stream update for '{}' ({}): {}", self.name, id, output);
                // Tools that support streaming can override this
            }
        }
        
        Ok(())
    }
}

impl BaseToolActor {
    pub fn new(name: impl Into<String>, config: Config) -> Self {
        Self {
            name: name.into(),
            config,
        }
    }
}