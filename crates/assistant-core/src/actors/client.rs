use ractor::{Actor, ActorRef, ActorProcessingErr};
use crate::config::Config;

/// Actor for OpenAI API communication
pub struct ClientActor {
    config: Config,
    client: reqwest::Client,
}

#[derive(Debug)]
pub enum ClientMessage {
    /// Generate completion
    Generate {
        prompt: String,
        system: Option<String>,
    },
    
    /// Cancel ongoing generation
    Cancel,
}

impl Actor for ClientActor {
    type Msg = ClientMessage;
    type State = ();
    type Arguments = Config;
    
    async fn pre_start(
        &self,
        _myself: ActorRef<Self::Msg>,
        _config: Self::Arguments,
    ) -> Result<Self::State, ActorProcessingErr> {
        tracing::info!("Client actor starting with model: {}", self.config.model);
        Ok(())
    }
    
    async fn handle(
        &self,
        _myself: ActorRef<Self::Msg>,
        msg: Self::Msg,
        _state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        match msg {
            ClientMessage::Generate { prompt, system } => {
                tracing::info!("Generating completion");
                // TODO: Implement OpenAI streaming
            }
            
            ClientMessage::Cancel => {
                tracing::info!("Cancelling generation");
                // TODO: Implement cancellation
            }
        }
        
        Ok(())
    }
}

impl ClientActor {
    pub fn new(config: Config) -> Self {
        Self {
            config,
            client: reqwest::Client::new(),
        }
    }
}