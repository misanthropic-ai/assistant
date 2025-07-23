use ractor::{Actor, ActorRef, ActorProcessingErr};
use std::collections::VecDeque;
use crate::config::Config;
use crate::messages::ChatMessage;

/// Main chat actor managing conversation flow
pub struct ChatActor {
    config: Config,
}

/// Chat actor state
pub struct ChatState {
    history: VecDeque<ChatMessage>,
    max_history: usize,
}

impl Actor for ChatActor {
    type Msg = ChatMessage;
    type State = ChatState;
    type Arguments = Config;
    
    async fn pre_start(
        &self,
        _myself: ActorRef<Self::Msg>,
        _config: Self::Arguments,
    ) -> Result<Self::State, ActorProcessingErr> {
        tracing::info!("Chat actor starting");
        Ok(ChatState {
            history: VecDeque::new(),
            max_history: 100,
        })
    }
    
    async fn handle(
        &self,
        _myself: ActorRef<Self::Msg>,
        msg: Self::Msg,
        state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        match msg {
            ChatMessage::UserPrompt { id, prompt } => {
                tracing::info!("Received user prompt: {}", prompt);
                state.history.push_back(ChatMessage::UserPrompt { id, prompt: prompt.clone() });
                // TODO: Send to ClientActor for processing
            }
            
            ChatMessage::StreamToken { token } => {
                // TODO: Forward to UI
            }
            
            ChatMessage::ToolRequest { id, call } => {
                tracing::info!("Tool request: {}", call.tool_name);
                // TODO: Route to appropriate tool actor
            }
            
            ChatMessage::ToolResult { id, result } => {
                tracing::info!("Tool result received");
                // TODO: Process and continue conversation
            }
            
            ChatMessage::Complete { id, response } => {
                tracing::info!("Response complete");
                // TODO: Update history and notify UI
            }
            
            ChatMessage::Error { id, error } => {
                tracing::error!("Error in chat: {}", error);
                // TODO: Handle error
            }
        }
        
        Ok(())
    }
}

impl ChatActor {
    pub fn new(config: Config) -> Self {
        Self {
            config,
        }
    }
}