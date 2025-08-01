use ractor::{Actor, ActorRef, ActorProcessingErr};
use std::collections::HashMap;
use crate::config::Config;
use crate::messages::{DelegatorMessage, ChatMessage};
use crate::actors::tools::ToolMessage;
use crate::actors::sub_agent::{SubAgentActor, SubAgentMessage};
use uuid::Uuid;

/// Actor that routes tools to specialized LLMs
pub struct DelegatorActor {
    config: Config,
}

/// Delegator state
pub struct DelegatorState {
    /// Map of tool names to their local tool actors
    tool_actors: HashMap<String, ActorRef<ToolMessage>>,
    /// Map of tool names to their sub-agent actors
    sub_agents: HashMap<String, ActorRef<SubAgentMessage>>,
    
    /// Active delegated requests
    active_requests: HashMap<Uuid, ActorRef<ChatMessage>>,
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
        
        let mut sub_agents = HashMap::new();
        
        // Create sub-agent actors for tools that need delegation
        for (tool_name, tool_config) in &config.tools.configs {
            if tool_config.should_delegate() {
                tracing::info!("Creating sub-agent for tool: {}", tool_name);
                
                let sub_agent = SubAgentActor::new(
                    tool_name.clone(),
                    tool_config.clone(),
                    config.clone(),
                );
                
                let (sub_agent_ref, _) = Actor::spawn(
                    Some(format!("sub-agent-{}", tool_name)),
                    sub_agent,
                    (),
                ).await?;
                
                sub_agents.insert(tool_name.clone(), sub_agent_ref);
            }
        }
        
        Ok(DelegatorState {
            tool_actors: HashMap::new(),
            sub_agents,
            active_requests: HashMap::new(),
        })
    }
    
    async fn handle(
        &self,
        myself: ActorRef<Self::Msg>,
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
                    .map(|tc| tc.should_delegate())
                    .unwrap_or(false);
                
                if should_delegate {
                    // Route to sub-agent
                    if let Some(sub_agent_ref) = state.sub_agents.get(&call.tool_name) {
                        tracing::info!("Delegating {} to sub-agent for request {}", call.tool_name, id);
                        
                        // Store the chat reference for later response
                        state.active_requests.insert(id, chat_ref.clone());
                        
                        // Extract the query from parameters
                        let query = if call.tool_name == "web_search" {
                            call.parameters.get("query")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string()
                        } else if call.tool_name == "knowledge_agent" {
                            // For knowledge_agent, extract based on action type
                            if let Some(action) = call.parameters.get("action").and_then(|v| v.as_str()) {
                                match action {
                                    "search" => call.parameters.get("query")
                                        .and_then(|v| v.as_str())
                                        .unwrap_or("")
                                        .to_string(),
                                    "analyze" | "synthesize" => call.parameters.get("topic")
                                        .and_then(|v| v.as_str())
                                        .unwrap_or("")
                                        .to_string(),
                                    _ => serde_json::to_string(&call.parameters).unwrap_or_default()
                                }
                            } else {
                                serde_json::to_string(&call.parameters).unwrap_or_default()
                            }
                        } else {
                            // For other tools, convert the entire parameters to a query
                            serde_json::to_string(&call.parameters).unwrap_or_default()
                        };
                        
                        tracing::info!("Sending query to sub-agent {}: {}", call.tool_name, query);
                        
                        // Send to sub-agent
                        sub_agent_ref.send_message(SubAgentMessage::ExecuteQuery {
                            id,
                            query,
                            reply_to: myself.clone(),
                        })?;
                    } else {
                        tracing::error!("Sub-agent not found for tool: {}", call.tool_name);
                        chat_ref.send_message(ChatMessage::Error {
                            id,
                            error: format!("Sub-agent not found for: {}", call.tool_name),
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
            
            DelegatorMessage::SubAgentResponse { id, result } => {
                tracing::info!("Received sub-agent response for request {}: {}", id, result);
                
                // Get the chat reference for this request
                if let Some(chat_ref) = state.active_requests.remove(&id) {
                    tracing::info!("Forwarding sub-agent result to chat actor for request {}", id);
                    // Send the result back to the chat actor
                    chat_ref.send_message(ChatMessage::ToolResult {
                        id,
                        result,
                    })?;
                } else {
                    tracing::warn!("No active request found for sub-agent response {}", id);
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