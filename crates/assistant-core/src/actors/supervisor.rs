use ractor::{Actor, ActorRef, ActorProcessingErr};
use std::collections::HashMap;
use uuid::Uuid;
use crate::config::Config;
use crate::messages::SupervisorMessage;

/// Root supervisor actor managing the actor tree
pub struct SupervisorActor {
    config: Config,
}

/// Actors for a single session
pub struct SessionActors {
    chat: ActorRef<crate::messages::ChatMessage>,
    // TODO: Add other session-specific actors
}

/// Supervisor state
pub struct SupervisorState {
    sessions: HashMap<Uuid, SessionActors>,
}

impl Actor for SupervisorActor {
    type Msg = SupervisorMessage;
    type State = SupervisorState;
    type Arguments = Config;
    
    async fn pre_start(
        &self,
        _myself: ActorRef<Self::Msg>,
        config: Self::Arguments,
    ) -> Result<Self::State, ActorProcessingErr> {
        tracing::info!("Supervisor actor starting");
        
        // TODO: Initialize global actors (tool registry, etc.)
        
        Ok(SupervisorState {
            sessions: HashMap::new(),
        })
    }
    
    async fn handle(
        &self,
        _myself: ActorRef<Self::Msg>,
        msg: Self::Msg,
        state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        match msg {
            SupervisorMessage::StartSession { session_id } => {
                tracing::info!("Starting session {}", session_id);
                // TODO: Spawn session actors
            }
            
            SupervisorMessage::EndSession { session_id } => {
                tracing::info!("Ending session {}", session_id);
                // TODO: Clean up session actors
                state.sessions.remove(&session_id);
            }
            
            SupervisorMessage::ReloadConfig => {
                tracing::info!("Reloading configuration");
                // TODO: Reload config and update actors
            }
            
            SupervisorMessage::GetStatus => {
                tracing::info!("Getting system status");
                // TODO: Collect and return status
            }
        }
        
        Ok(())
    }
    
    async fn post_stop(
        &self,
        _myself: ActorRef<Self::Msg>,
        _state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        tracing::info!("Supervisor actor stopping");
        Ok(())
    }
}

impl SupervisorActor {
    pub fn new(config: Config) -> Self {
        Self {
            config,
        }
    }
}