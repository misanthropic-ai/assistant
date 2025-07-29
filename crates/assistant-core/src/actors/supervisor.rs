use ractor::{Actor, ActorRef, ActorProcessingErr};
use std::collections::HashMap;
use uuid::Uuid;
use crate::config::Config;
use crate::messages::{SupervisorMessage, ChatMessage};
use crate::actors::{
    chat::ChatActor,
    client::{ClientActor, ClientMessage},
    delegator::DelegatorActor,
};
use crate::messages::DelegatorMessage;
use crate::actors::tools::{ToolRegistry, ToolMessage};

/// Root supervisor actor managing the actor tree
pub struct SupervisorActor {
    config: Config,
}

/// Actors for a single session
pub struct SessionActors {
    chat: ActorRef<ChatMessage>,
    client: ActorRef<ClientMessage>,
    delegator: ActorRef<DelegatorMessage>,
}

/// Supervisor state
pub struct SupervisorState {
    sessions: HashMap<Uuid, SessionActors>,
    tool_actors: HashMap<String, ActorRef<ToolMessage>>,
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
        
        // Initialize tool registry and create tool actors
        let registry = ToolRegistry::new(config.clone());
        let tool_actors = registry.initialize_tools().await
            .map_err(|e| format!("Failed to initialize tools: {}", e))?;
        
        tracing::info!("Initialized {} tools: {:?}", 
            tool_actors.len(), 
            tool_actors.keys().collect::<Vec<_>>()
        );
        
        Ok(SupervisorState {
            sessions: HashMap::new(),
            tool_actors,
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
                
                // Create client actor
                let client_actor = ClientActor::new(self.config.clone());
                let (client_ref, _) = Actor::spawn(
                    Some(format!("client-{}", session_id)),
                    client_actor,
                    self.config.clone()
                ).await?;
                
                // Create chat actor
                let chat_actor = ChatActor::new(self.config.clone())
                    .with_client_ref(client_ref.clone());
                let (chat_ref, _) = Actor::spawn(
                    Some(format!("chat-{}", session_id)),
                    chat_actor,
                    self.config.clone()
                ).await?;
                
                // Create delegator actor
                let delegator_actor = DelegatorActor::new(self.config.clone());
                let (delegator_ref, _) = Actor::spawn(
                    Some(format!("delegator-{}", session_id)),
                    delegator_actor,
                    self.config.clone()
                ).await?;
                
                // Update chat actor with delegator reference
                chat_ref.send_message(ChatMessage::SetDelegatorRef(delegator_ref.clone()))?;
                
                // Update client actor with chat reference
                client_ref.send_message(ClientMessage::SetChatRef(chat_ref.clone()))?;
                
                // Update delegator with tool actors
                for (tool_name, tool_ref) in &state.tool_actors {
                    delegator_ref.send_message(DelegatorMessage::RegisterTool {
                        name: tool_name.clone(),
                        actor_ref: tool_ref.clone(),
                    })?;
                }
                
                // Store session actors
                state.sessions.insert(session_id, SessionActors {
                    chat: chat_ref,
                    client: client_ref,
                    delegator: delegator_ref,
                });
            }
            
            SupervisorMessage::EndSession { session_id } => {
                tracing::info!("Ending session {}", session_id);
                
                if let Some(session) = state.sessions.remove(&session_id) {
                    // Send cancel to client
                    let _ = session.client.send_message(ClientMessage::Cancel);
                    
                    // Stop actors
                    let _ = session.chat.stop(None);
                    let _ = session.client.stop(None);
                    let _ = session.delegator.stop(None);
                }
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