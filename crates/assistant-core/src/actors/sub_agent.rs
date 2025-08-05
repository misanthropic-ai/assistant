use ractor::{Actor, ActorRef, ActorProcessingErr};
use crate::config::Config;
use crate::config::tool_config::ToolConfig;
use crate::messages::{ChatMessage, DisplayContext, DelegatorMessage, ToolMessage};
use crate::actors::client::{ClientActor, ClientMessage};
use crate::actors::sub_agent_chat::SubAgentChatActor;
use crate::actors::tools::web_search::WebSearchActor;
use crate::actors::tools::web_fetch::WebFetchActor;
use crate::actors::sub_agent_display::SubAgentDisplay;
use uuid::Uuid;
use std::collections::HashMap;
use tokio::sync::oneshot;

/// Messages for sub-agent communication
#[derive(Debug, Clone)]
pub enum SubAgentMessage {
    /// Execute a query with the sub-agent
    ExecuteQuery {
        id: Uuid,
        query: String,
        reply_to: ActorRef<DelegatorMessage>,
    },
    
    /// Forward a chat message from our internal chat actor
    ForwardChatMessage(ChatMessage),
}

/// Sub-agent actor that encapsulates a complete agent with limited tools
pub struct SubAgentActor {
    tool_name: String,
    tool_config: ToolConfig,
    base_config: Config,
}

/// Sub-agent state
pub struct SubAgentState {
    /// Reference to our own client actor
    #[allow(dead_code)]
    client_ref: Option<ActorRef<ClientMessage>>,
    
    /// Reference to our own chat actor
    chat_ref: Option<ActorRef<ChatMessage>>,
    
    /// Tool actors available to this sub-agent
    #[allow(dead_code)]
    tool_actors: HashMap<String, ActorRef<ToolMessage>>,
    
    /// Active requests
    #[allow(dead_code)]
    active_requests: HashMap<Uuid, oneshot::Sender<String>>,
    
    /// Reply-to references for each request
    reply_refs: HashMap<Uuid, ActorRef<DelegatorMessage>>,
}

impl Actor for SubAgentActor {
    type Msg = SubAgentMessage;
    type State = SubAgentState;
    type Arguments = ();
    
    async fn pre_start(
        &self,
        myself: ActorRef<Self::Msg>,
        _: Self::Arguments,
    ) -> Result<Self::State, ActorProcessingErr> {
        tracing::info!("Sub-agent actor starting for tool: {}", self.tool_name);
        
        // Create a custom config for this sub-agent
        let mut sub_config = self.base_config.clone();
        
        // Override with tool-specific LLM settings
        if let Some(api_key) = &self.tool_config.api_key {
            sub_config.api_key = api_key.clone();
        }
        if let Some(base_url) = &self.tool_config.base_url {
            sub_config.base_url = base_url.clone();
        }
        if let Some(model) = &self.tool_config.model {
            sub_config.model = model.clone();
        }
        if let Some(temperature) = self.tool_config.temperature {
            sub_config.temperature = temperature;
        }
        
        // Create sub-agent's own client actor
        let client_actor = ClientActor::new(sub_config.clone());
        let (client_ref, _) = Actor::spawn(
            Some(format!("sub-client-{}", self.tool_name)),
            client_actor,
            sub_config.clone(),
        ).await?;
        
        // Create and register limited tool actors based on the sub-agent type
        let mut tool_actors = HashMap::new();
        
        // Knowledge agent gets memory tool in addition to web tools
        if self.tool_name == "knowledge_agent" {
            tracing::info!("Creating memory tool for knowledge_agent subagent");
            // Create memory actor for knowledge agent
            let memory_actor = match crate::actors::tools::memory::MemoryActor::new(sub_config.clone()).await {
                Ok(actor) => actor,
                Err(e) => {
                    tracing::error!("Failed to create memory actor for knowledge agent: {}", e);
                    return Err(ActorProcessingErr::from(format!("Failed to create memory actor: {}", e)));
                }
            };
            let (memory_ref, _) = Actor::spawn(
                Some(format!("sub-memory-{}", self.tool_name)),
                memory_actor,
                sub_config.clone(),
            ).await?;
            tool_actors.insert("memory".to_string(), memory_ref.clone());
            tracing::info!("Memory tool created for knowledge_agent");
        }
        
        // Computer use agent gets screenshot and desktop_control tools in addition to web tools
        if self.tool_name == "computer_use" {
            // Create screenshot actor for computer use agent
            let screenshot_actor = crate::actors::tools::screenshot::ScreenshotActor::new(sub_config.clone());
            let (screenshot_ref, _) = Actor::spawn(
                Some(format!("sub-screenshot-{}", self.tool_name)),
                screenshot_actor,
                sub_config.clone(),
            ).await?;
            tool_actors.insert("screenshot".to_string(), screenshot_ref.clone());
            
            // Create desktop control actor for computer use agent
            let desktop_control_actor = crate::actors::tools::desktop_control::DesktopControlActor::new(sub_config.clone());
            let (desktop_control_ref, _) = Actor::spawn(
                Some(format!("sub-desktop-control-{}", self.tool_name)),
                desktop_control_actor,
                sub_config.clone(),
            ).await?;
            tool_actors.insert("desktop_control".to_string(), desktop_control_ref.clone());
        }
        
        // All sub-agents get web_search and web_fetch
        let web_search_actor = WebSearchActor::new(sub_config.clone());
        let (web_search_ref, _) = Actor::spawn(
            Some(format!("sub-web-search-{}", self.tool_name)),
            web_search_actor,
            sub_config.clone(),
        ).await?;
        tool_actors.insert("web_search".to_string(), web_search_ref.clone());
        
        let web_fetch_actor = WebFetchActor::new(sub_config.clone());
        let (web_fetch_ref, _) = Actor::spawn(
            Some(format!("sub-web-fetch-{}", self.tool_name)),
            web_fetch_actor,
            sub_config.clone(),
        ).await?;
        tool_actors.insert("web_fetch".to_string(), web_fetch_ref.clone());
        
        // Determine if the OpenAI "tools" API should be used for this sub-agent
        let enable_tool_api = self.tool_config.use_tool_api;

        // Create sub-agent's own chat actor with tool actors
        let chat_actor = SubAgentChatActor::new(sub_config.clone(), tool_actors.clone(), enable_tool_api)
            .with_client_ref(client_ref.clone());
        let (chat_ref, _) = Actor::spawn(
            Some(format!("sub-chat-{}", self.tool_name)),
            chat_actor,
            sub_config.clone(),
        ).await?;
        
        // Set up actor references
        client_ref.send_message(ClientMessage::SetChatRef(chat_ref.clone()))?;
        
        // Create display forwarder actor
        let display_actor = SubAgentDisplay::new();
        let (display_ref, _) = Actor::spawn(
            Some(format!("sub-display-{}", self.tool_name)),
            display_actor,
            myself.clone(),
        ).await?;
        
        // Register display with chat actor
        chat_ref.send_message(ChatMessage::RegisterDisplay {
            context: DisplayContext::SubAgent,
            display_ref,
        })?;
        
        Ok(SubAgentState {
            client_ref: Some(client_ref),
            chat_ref: Some(chat_ref),
            tool_actors,
            active_requests: HashMap::new(),
            reply_refs: HashMap::new(),
        })
    }
    
    async fn handle(
        &self,
        _myself: ActorRef<Self::Msg>,
        msg: Self::Msg,
        state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        match msg {
            SubAgentMessage::ExecuteQuery { id, query, reply_to } => {
                tracing::warn!("Sub-agent {} executing query: {}", self.tool_name, query);
                tracing::info!("Sub-agent {} request ID: {}", self.tool_name, id);
                
                // Store the reply reference
                state.reply_refs.insert(id, reply_to);
                tracing::debug!("Sub-agent {} stored reply ref for request {}", self.tool_name, id);
                
                // Prepare the prompt with system context
                let full_prompt = if let Some(system_prompt) = &self.tool_config.system_prompt {
                    // Include system prompt context in the user message
                    format!("Context: {}\n\nTask: {}", system_prompt, query)
                } else {
                    query
                };
                
                tracing::info!("Sub-agent {} sending to chat: {}", self.tool_name, full_prompt);
                
                // Send the query to our chat actor
                if let Some(ref chat_ref) = state.chat_ref {
                    chat_ref.send_message(ChatMessage::UserPrompt {
                        id,
                        content: crate::messages::UserMessageContent::Text(full_prompt),
                        context: DisplayContext::SubAgent,
                    })?;
                } else {
                    tracing::error!("Sub-agent {} has no chat_ref!", self.tool_name);
                }
            }
            
            SubAgentMessage::ForwardChatMessage(chat_msg) => {
                match chat_msg {
                    ChatMessage::Complete { id, response } => {
                        tracing::info!("Sub-agent {} completed request {}: {}", self.tool_name, id, response);
                        // This is the final response, send it back to the delegator
                        if let Some(reply_ref) = state.reply_refs.remove(&id) {
                            tracing::info!("Sending response back to delegator for request {}", id);
                            reply_ref.send_message(DelegatorMessage::SubAgentResponse {
                                id,
                                result: response,
                            })?;
                        } else {
                            tracing::error!("No reply reference found for completed request {}", id);
                        }
                    }
                    ChatMessage::Error { id, error } => {
                        tracing::error!("Sub-agent error for request {}: {}", id, error);
                        if let Some(reply_ref) = state.reply_refs.remove(&id) {
                            reply_ref.send_message(DelegatorMessage::SubAgentResponse {
                                id,
                                result: format!("Error: {}", error),
                            })?;
                        }
                    }
                    ChatMessage::StreamToken { token } => {
                        // Could accumulate tokens if needed
                        tracing::trace!("Sub-agent stream token: {}", token);
                    }
                    ChatMessage::ToolRequest { .. } => {
                        // Tool requests are handled by SubAgentChatActor directly
                        tracing::trace!("Sub-agent forwarding tool request");
                    }
                    ChatMessage::ToolResult { .. } => {
                        // Tool results will be sent to chat actor
                        tracing::trace!("Sub-agent received tool result");
                    }
                    _ => {
                        // Ignore other messages
                    }
                }
            }
        }
        
        Ok(())
    }
}

impl SubAgentActor {
    pub fn new(tool_name: String, tool_config: ToolConfig, base_config: Config) -> Self {
        Self {
            tool_name,
            tool_config,
            base_config,
        }
    }
}

