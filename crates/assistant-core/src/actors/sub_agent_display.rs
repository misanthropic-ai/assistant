use ractor::{Actor, ActorRef, ActorProcessingErr};
use crate::messages::ChatMessage;
use crate::actors::sub_agent::SubAgentMessage;

/// A simple display actor that forwards ChatMessages to SubAgent
pub struct SubAgentDisplay;

pub struct SubAgentDisplayState {
    sub_agent_ref: ActorRef<SubAgentMessage>,
}

impl Actor for SubAgentDisplay {
    type Msg = ChatMessage;
    type State = SubAgentDisplayState;
    type Arguments = ActorRef<SubAgentMessage>;
    
    async fn pre_start(
        &self,
        _myself: ActorRef<Self::Msg>,
        sub_agent_ref: Self::Arguments,
    ) -> Result<Self::State, ActorProcessingErr> {
        Ok(SubAgentDisplayState { sub_agent_ref })
    }
    
    async fn handle(
        &self,
        _myself: ActorRef<Self::Msg>,
        msg: Self::Msg,
        state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        tracing::debug!("SubAgentDisplay forwarding message: {:?}", msg);
        // Forward the chat message to the sub-agent
        state.sub_agent_ref.send_message(SubAgentMessage::ForwardChatMessage(msg))?;
        Ok(())
    }
}

impl SubAgentDisplay {
    pub fn new() -> Self {
        Self
    }
}