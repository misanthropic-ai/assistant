use assistant_core::{
    messages::ChatMessage,
    ractor::{Actor, ActorRef, ActorProcessingErr},
};
use tokio::sync::mpsc;

/// Actor that receives chat messages and forwards them to the TUI
pub struct DisplayActor {
    /// Channel to send messages to the TUI
    tui_tx: mpsc::UnboundedSender<ChatMessage>,
}

impl DisplayActor {
    pub fn new(tui_tx: mpsc::UnboundedSender<ChatMessage>) -> Self {
        Self { tui_tx }
    }
}

impl Actor for DisplayActor {
    type Msg = ChatMessage;
    type State = ();
    type Arguments = mpsc::UnboundedSender<ChatMessage>;
    
    async fn pre_start(
        &self,
        myself: ActorRef<Self::Msg>,
        _tui_tx: Self::Arguments,
    ) -> Result<Self::State, ActorProcessingErr> {
        tracing::info!("DisplayActor started with ref: {:?}", myself);
        Ok(())
    }
    
    async fn handle(
        &self,
        _myself: ActorRef<Self::Msg>,
        msg: Self::Msg,
        _state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        // Forward all messages to the TUI
        if let Err(e) = self.tui_tx.send(msg.clone()) {
            tracing::error!("Failed to send message to TUI: {}", e);
        }
        
        Ok(())
    }
}