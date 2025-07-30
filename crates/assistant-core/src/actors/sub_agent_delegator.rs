use ractor::{Actor, ActorRef, ActorProcessingErr};
use std::collections::HashMap;
use crate::messages::{DelegatorMessage, ChatMessage, ToolMessage};
use crate::actors::sub_agent::{SubAgentActor, SubAgentMessage};
use uuid::Uuid;

/// A simple delegator for sub-agents that forwards tool calls back to the sub-agent
pub struct SubAgentDelegator {
    sub_agent_ref: ActorRef<SubAgentMessage>,
}

pub struct SubAgentDelegatorState {
    /// Map of active tool requests to track them
    active_requests: HashMap<Uuid, ActorRef<ChatMessage>>,
}

impl Actor for SubAgentDelegator {
    type Msg = DelegatorMessage;
    type State = SubAgentDelegatorState;
    type Arguments = ActorRef<SubAgentMessage>;
    
    async fn pre_start(
        &self,
        _myself: ActorRef<Self::Msg>,
        _: Self::Arguments,
    ) -> Result<Self::State, ActorProcessingErr> {
        tracing::info!("Sub-agent delegator starting");
        
        Ok(SubAgentDelegatorState {
            active_requests: HashMap::new(),
        })
    }
    
    async fn handle(
        &self,
        _myself: ActorRef<Self::Msg>,
        msg: Self::Msg,
        state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        match msg {
            DelegatorMessage::RouteToolCall { id, call, chat_ref } => {
                tracing::info!("Sub-agent delegator routing tool call: {} back to sub-agent", call.tool_name);
                
                // Store the chat reference
                state.active_requests.insert(id, chat_ref);
                
                // Forward the tool request to the sub-agent for handling
                self.sub_agent_ref.send_message(SubAgentMessage::ForwardChatMessage(
                    ChatMessage::ToolRequest { id, call }
                ))?;
            }
            
            _ => {
                // Ignore other messages
            }
        }
        
        Ok(())
    }
}

impl SubAgentDelegator {
    pub fn new(sub_agent_ref: ActorRef<SubAgentMessage>) -> Self {
        Self { sub_agent_ref }
    }
}