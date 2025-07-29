use ractor::{Actor, ActorRef, ActorProcessingErr};
use std::collections::HashMap;
use crate::config::Config;
use crate::messages::{DelegatorMessage, ToolCall, ChatMessage};
use crate::actors::tools::ToolMessage;
use crate::actors::client::{ClientActor, ClientMessage};
use uuid::Uuid;

/// Actor that routes tools to specialized LLMs
pub struct DelegatorActor {
    config: Config,
}

/// Delegator state
pub struct DelegatorState {
    /// Map of tool names to their local tool actors
    tool_actors: HashMap<String, ActorRef<ToolMessage>>,
    /// Map of tool names to their delegated client actors
    delegated_clients: HashMap<String, ActorRef<ClientMessage>>,
}

impl Actor for DelegatorActor {
    type Msg = DelegatorMessage;
    type State = DelegatorState;
    type Arguments = Config;
    
    async fn pre_start(
        &self,
        _myself: ActorRef<Self::Msg>,
        config: Self::Arguments,
    ) -> Result<Self::State, ActorProcessingErr> {
        tracing::info!("Delegator actor starting");
        
        // TODO: Create client actors for each delegated tool based on config
        let mut delegated_clients = HashMap::new();
        
        // Create delegated client actors for tools that need specialized LLMs
        for (tool_name, tool_config) in &config.tools.configs {
            if tool_config.delegate && tool_config.llm_config.is_some() {
                // TODO: Create specialized client actor
                tracing::info!("Tool {} is configured for delegation", tool_name);
            }
        }
        
        Ok(DelegatorState {
            tool_actors: HashMap::new(),
            delegated_clients,
        })
    }
    
    async fn handle(
        &self,
        _myself: ActorRef<Self::Msg>,
        msg: Self::Msg,
        state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        match msg {
            DelegatorMessage::RegisterTool { name, actor_ref } => {
                tracing::info!("Registering tool: {}", name);
                state.tool_actors.insert(name, actor_ref);
            }
            
            DelegatorMessage::RouteToolCall { id, call, chat_ref } => {
                tracing::info!("Routing tool call: {}", call.tool_name);
                
                // Check if tool should be delegated
                let tool_config = self.config.tools.configs.get(&call.tool_name);
                let should_delegate = tool_config
                    .map(|tc| tc.delegate && tc.llm_config.is_some())
                    .unwrap_or(false);
                
                if should_delegate {
                    // Route to delegated client
                    if let Some(delegated_client) = state.delegated_clients.get(&call.tool_name) {
                        // TODO: Format tool call as specialized prompt
                        tracing::info!("Delegating {} to specialized LLM", call.tool_name);
                        // For now, just return an error
                        chat_ref.send_message(ChatMessage::Error {
                            id,
                            error: format!("Tool delegation not yet implemented for: {}", call.tool_name),
                        })?;
                    } else {
                        chat_ref.send_message(ChatMessage::Error {
                            id,
                            error: format!("Delegated client not found for: {}", call.tool_name),
                        })?;
                    }
                } else {
                    // Route to local tool actor
                    if let Some(tool_ref) = state.tool_actors.get(&call.tool_name) {
                        // Execute tool locally
                        tool_ref.send_message(ToolMessage::Execute {
                            id,
                            params: call.parameters,
                            chat_ref: chat_ref.clone(),
                        })?;
                    } else {
                        chat_ref.send_message(ChatMessage::Error {
                            id,
                            error: format!("Tool not found: {}", call.tool_name),
                        })?;
                    }
                }
            }
        }
        
        Ok(())
    }
}

impl DelegatorActor {
    pub fn new(config: Config) -> Self {
        Self {
            config,
        }
    }
}