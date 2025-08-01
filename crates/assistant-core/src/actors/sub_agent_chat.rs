use ractor::{Actor, ActorRef, ActorProcessingErr};
use std::collections::{VecDeque, HashMap};
use crate::config::Config;
use crate::messages::{ChatMessage, DisplayContext, ToolMessage};
use crate::actors::client::ClientMessage;
use crate::openai_compat::{ChatMessage as OpenAIMessage, Tool, FunctionDef, UserContent};
use uuid::Uuid;

/// Simplified chat actor for sub-agents that calls tools directly
pub struct SubAgentChatActor {
    config: Config,
    client_ref: Option<ActorRef<ClientMessage>>,
    tool_actors: HashMap<String, ActorRef<ToolMessage>>,
}

/// SubAgentChat actor state
pub struct SubAgentChatState {
    history: VecDeque<ChatMessage>,
    messages: Vec<OpenAIMessage>,
    max_history: usize,
    current_request: Option<Uuid>,
    current_context: Option<DisplayContext>,
    display_refs: HashMap<DisplayContext, ActorRef<ChatMessage>>,
    pending_tool_calls: HashMap<Uuid, (String, ActorRef<ChatMessage>)>,
}

impl Actor for SubAgentChatActor {
    type Msg = ChatMessage;
    type State = SubAgentChatState;
    type Arguments = Config;
    
    async fn pre_start(
        &self,
        _myself: ActorRef<Self::Msg>,
        _config: Self::Arguments,
    ) -> Result<Self::State, ActorProcessingErr> {
        tracing::info!("SubAgentChat actor starting");
        Ok(SubAgentChatState {
            history: VecDeque::new(),
            messages: Vec::new(),
            max_history: 100,
            current_request: None,
            current_context: None,
            display_refs: HashMap::new(),
            pending_tool_calls: HashMap::new(),
        })
    }
    
    async fn handle(
        &self,
        myself: ActorRef<Self::Msg>,
        msg: Self::Msg,
        state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        // Send to display actors if applicable
        match &msg {
            ChatMessage::StreamToken { .. } |
            ChatMessage::ToolRequest { .. } |
            ChatMessage::ToolResult { .. } |
            ChatMessage::AssistantResponse { .. } |
            ChatMessage::Complete { .. } |
            ChatMessage::Error { .. } => {
                if let Some(context) = &state.current_context {
                    if let Some(display_ref) = state.display_refs.get(context) {
                        let _ = display_ref.send_message(msg.clone());
                    }
                }
            }
            _ => {}
        }
        
        match msg {
            ChatMessage::UserPrompt { id, content, context } => {
                // Extract text for logging
                let prompt_text = match &content {
                    crate::messages::UserMessageContent::Text(text) => text.clone(),
                    crate::messages::UserMessageContent::MultiModal { text, .. } => text.clone(),
                };
                
                tracing::info!("SubAgentChat received user prompt: {}", prompt_text);
                state.current_context = Some(context.clone());
                state.history.push_back(ChatMessage::UserPrompt { id, content: content.clone(), context });
                state.current_request = Some(id);
                
                // Add user message to conversation
                let user_msg = OpenAIMessage::User {
                    content: match content {
                        crate::messages::UserMessageContent::Text(text) => UserContent::Text(text),
                        crate::messages::UserMessageContent::MultiModal { text, images } => {
                            let mut parts = vec![
                                crate::openai_compat::ContentPart::Text { text },
                            ];
                            for image_url in images {
                                parts.push(crate::openai_compat::ContentPart::Image {
                                    image_url: crate::openai_compat::ImageUrl {
                                        url: image_url,
                                        detail: None,
                                    },
                                });
                            }
                            UserContent::Array(parts)
                        }
                    },
                    name: None,
                };
                state.messages.push(user_msg);
                
                // Build tools list (only the tools we have actors for)
                let tools = self.build_tools();
                
                // Send to client for generation
                if let Some(ref client_ref) = self.client_ref {
                    client_ref.send_message(ClientMessage::Generate {
                        id,
                        messages: state.messages.clone(),
                        tools,
                    })?;
                } else {
                    return Err("Client actor not set".into());
                }
            }
            
            ChatMessage::StreamToken { token } => {
                tracing::debug!("Stream token: {}", token);
            }
            
            ChatMessage::ToolRequest { id, call } => {
                // This is now handled in AssistantResponse
                tracing::warn!("SubAgentChat received legacy ToolRequest message - this should be handled via AssistantResponse");
                state.history.push_back(ChatMessage::ToolRequest { id, call: call.clone() });
            }
            
            ChatMessage::ToolResult { id, result } => {
                tracing::info!("SubAgentChat tool result received for request {}: {}", id, result);
                state.history.push_back(ChatMessage::ToolResult { id, result: result.clone() });
                
                // Remove from pending
                state.pending_tool_calls.remove(&id);
                
                // Add tool result to messages
                let tool_msg = OpenAIMessage::Tool {
                    content: result,
                    tool_call_id: id.to_string(),
                };
                state.messages.push(tool_msg);
                
                // Continue conversation
                if let Some(ref client_ref) = self.client_ref {
                    tracing::info!("SubAgentChat continuing conversation after tool result for request {}", id);
                    let tools = self.build_tools();
                    client_ref.send_message(ClientMessage::Generate {
                        id,
                        messages: state.messages.clone(),
                        tools,
                    })?;
                } else {
                    tracing::error!("No client ref to continue conversation after tool result");
                }
            }
            
            ChatMessage::AssistantResponse { id, content, tool_calls } => {
                tracing::info!("SubAgentChat assistant response for request {}: content={:?}, tool_calls={}", 
                    id, content, tool_calls.len());
                
                // Store in history
                state.history.push_back(ChatMessage::AssistantResponse { 
                    id, 
                    content: content.clone(), 
                    tool_calls: tool_calls.clone() 
                });
                
                // Add assistant message with tool calls
                if content.is_some() || !tool_calls.is_empty() {
                    let openai_tool_calls = if tool_calls.is_empty() {
                        None
                    } else {
                        Some(tool_calls.iter().enumerate().map(|(_idx, call)| {
                            crate::openai_compat::ToolCall {
                                id: format!("call_{}", Uuid::new_v4()),
                                tool_type: "function".to_string(),
                                function: crate::openai_compat::FunctionCall {
                                    name: call.tool_name.clone(),
                                    arguments: call.parameters.to_string(),
                                },
                            }
                        }).collect())
                    };
                    
                    let assistant_msg = OpenAIMessage::Assistant {
                        content: content.clone(),
                        name: None,
                        tool_calls: openai_tool_calls,
                    };
                    state.messages.push(assistant_msg);
                }
                
                // Process any tool calls
                for call in tool_calls {
                    let tool_id = Uuid::new_v4();
                    
                    // Route tool call directly to the tool actor
                    if let Some(tool_ref) = self.tool_actors.get(&call.tool_name) {
                        state.pending_tool_calls.insert(tool_id, (call.tool_name.clone(), myself.clone()));
                        tool_ref.send_message(ToolMessage::Execute {
                            id: tool_id,
                            params: call.parameters,
                            chat_ref: myself.clone(),
                        })?;
                    } else {
                        tracing::error!("Tool actor not found: {}", call.tool_name);
                        myself.send_message(ChatMessage::ToolResult {
                            id: tool_id,
                            result: format!("Error: Tool '{}' not available", call.tool_name),
                        })?;
                    }
                }
            }
            
            ChatMessage::Complete { id, response } => {
                tracing::info!("SubAgentChat response complete");
                state.history.push_back(ChatMessage::Complete { id, response: response.clone() });
                
                // No longer add assistant message here - handled in AssistantResponse
                
                // Clear current request
                state.current_request = None;
                
                // Trim history if needed
                while state.history.len() > state.max_history {
                    state.history.pop_front();
                }
            }
            
            ChatMessage::Error { id, error } => {
                tracing::error!("Error in SubAgentChat: {}", error);
                state.history.push_back(ChatMessage::Error { id, error: error.clone() });
                state.current_request = None;
            }
            
            ChatMessage::RegisterDisplay { context, display_ref } => {
                tracing::debug!("Registering display actor for context: {:?}", context);
                state.display_refs.insert(context, display_ref);
            }
            
            // These messages are not used in SubAgentChat
            ChatMessage::SetDelegatorRef(_) => {
                tracing::debug!("SubAgentChat ignoring SetDelegatorRef - no delegator needed");
            }
            ChatMessage::SetPersistenceRef(_) => {
                tracing::debug!("SubAgentChat ignoring SetPersistenceRef - no persistence needed");
            }
        }
        
        Ok(())
    }
}

impl SubAgentChatActor {
    pub fn new(config: Config, tool_actors: HashMap<String, ActorRef<ToolMessage>>) -> Self {
        Self {
            config,
            client_ref: None,
            tool_actors,
        }
    }
    
    pub fn with_client_ref(mut self, client_ref: ActorRef<ClientMessage>) -> Self {
        self.client_ref = Some(client_ref);
        self
    }
    
    fn build_tools(&self) -> Vec<Tool> {
        let mut tools = Vec::new();
        
        // Only include tools we have actors for
        for tool_name in self.tool_actors.keys() {
            let tool = self.create_tool_definition(tool_name);
            tools.push(tool);
        }
        
        tools
    }
    
    fn create_tool_definition(&self, tool_name: &str) -> Tool {
        // Create tool definitions for the tools available to sub-agents
        let (description, parameters) = match tool_name {
            "web_search" => (
                "Search the web for information",
                serde_json::json!({
                    "type": "object",
                    "properties": {
                        "query": {
                            "type": "string",
                            "description": "The search query"
                        }
                    },
                    "required": ["query"]
                })
            ),
            "web_fetch" => (
                "Fetch content from a URL",
                serde_json::json!({
                    "type": "object",
                    "properties": {
                        "url": {
                            "type": "string",
                            "description": "The URL to fetch"
                        }
                    },
                    "required": ["url"]
                })
            ),
            _ => (
                "Unknown tool",
                serde_json::json!({
                    "type": "object",
                    "properties": {}
                })
            )
        };
        
        Tool {
            tool_type: "function".to_string(),
            function: FunctionDef {
                name: tool_name.to_string(),
                description: description.to_string(),
                parameters,
            },
        }
    }
}