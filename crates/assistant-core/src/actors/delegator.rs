use ractor::{Actor, ActorRef, ActorProcessingErr};
use std::collections::HashMap;
use crate::config::Config;
use crate::messages::ToolCall;

/// Actor that routes tools to specialized LLMs
pub struct DelegatorActor {
    config: Config,
}

/// Delegator state
pub struct DelegatorState {
    /// Map of tool names to their delegated client actors
    delegated_clients: HashMap<String, ActorRef<super::client::ClientMessage>>,
}

#[derive(Debug)]
pub enum DelegatorMessage {
    /// Register a tool for delegation
    RegisterTool {
        tool_name: String,
        client: ActorRef<super::client::ClientMessage>,
    },
    
    /// Execute a delegated tool
    ExecuteDelegated {
        tool_call: ToolCall,
    },
}

impl Actor for DelegatorActor {
    type Msg = DelegatorMessage;
    type State = DelegatorState;
    type Arguments = Config;
    
    async fn pre_start(
        &self,
        _myself: ActorRef<Self::Msg>,
        _config: Self::Arguments,
    ) -> Result<Self::State, ActorProcessingErr> {
        tracing::info!("Delegator actor starting");
        
        // TODO: Create client actors for each delegated tool based on config
        
        Ok(DelegatorState {
            delegated_clients: HashMap::new(),
        })
    }
    
    async fn handle(
        &self,
        _myself: ActorRef<Self::Msg>,
        msg: Self::Msg,
        state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        match msg {
            DelegatorMessage::RegisterTool { tool_name, client } => {
                tracing::info!("Registering delegated tool: {}", tool_name);
                state.delegated_clients.insert(tool_name, client);
            }
            
            DelegatorMessage::ExecuteDelegated { tool_call } => {
                tracing::info!("Executing delegated tool: {}", tool_call.tool_name);
                // TODO: Route to appropriate delegated client
                // TODO: Format specialized prompt
                // TODO: Return results
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

// TODO: Implement delegation logic
// - Create separate ClientActor for each unique LLM config
// - Format tool requests as specialized prompts
// - Handle streaming responses
// - Aggregate results back to main conversation