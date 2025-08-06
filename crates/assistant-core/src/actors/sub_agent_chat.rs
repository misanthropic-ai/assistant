use ractor::{Actor, ActorRef, ActorProcessingErr};
use std::collections::{VecDeque, HashMap};
use crate::config::Config;
use crate::messages::{ChatMessage, DisplayContext, ToolMessage};
use crate::actors::client::ClientMessage;
use crate::openai_compat::{ChatMessage as OpenAIMessage, Tool, FunctionDef, UserContent};
use uuid::Uuid;

/// Simplified chat actor for sub-agents that calls tools directly
pub struct SubAgentChatActor {
    #[allow(dead_code)]
    config: Config,
    client_ref: Option<ActorRef<ClientMessage>>,
    tool_actors: HashMap<String, ActorRef<ToolMessage>>,
    enable_tool_api: bool,
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
            ChatMessage::UserPrompt { id, content, context, session_id: _ } => {
                // Extract text for logging
                let mut prompt_text = match &content {
                    crate::messages::UserMessageContent::Text(text) => text.clone(),
                    crate::messages::UserMessageContent::MultiModal { text, .. } => text.clone(),
                };
                
                // Prepend tool catalogue if function-calling API is disabled
                if !self.enable_tool_api {
                    let cat = self.build_tool_catalogue_xml();
                    prompt_text = format!("{}\n\n{}", cat, prompt_text);
                }

                tracing::info!("SubAgentChat received user prompt: {}", prompt_text);
                state.current_context = Some(context.clone());
                state.history.push_back(ChatMessage::UserPrompt { id, content: content.clone(), context, session_id: None });
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
                
                // Retrieve original tool name (if any) and remove from pending
                let tool_name_opt = state.pending_tool_calls.remove(&id).map(|(n, _)| n);
                
                if self.enable_tool_api {
                    // ----------------------------------------------------------
                    // Standard function-calling path – feed back via Tool message
                    // ----------------------------------------------------------
                    let tool_msg = OpenAIMessage::Tool {
                        content: result.clone(),
                        tool_call_id: id.to_string(),
                    };
                    state.messages.push(tool_msg);
                } else if let Some(tool_name) = &tool_name_opt {
                    // ----------------------------------------------------------
                    // XML shim path – wrap the JSON result in <tool_result>
                    // ----------------------------------------------------------
                    
                    // Special handling for screenshot tool results to avoid token limit issues
                    let processed_result = if tool_name == "screenshot" {
                        self.process_screenshot_result(&result)
                    } else {
                        result.clone()
                    };
                    
                    let wrapped = format!(
                        "<tool_result name=\"{}\">{}</tool_result>",
                        tool_name,
                        processed_result
                    );
                    state.messages.push(OpenAIMessage::Assistant {
                        content: Some(wrapped),
                        name: None,
                        tool_calls: None,
                    });
                }
                
                // Continue conversation - handle screenshot results specially
                if let Some(ref client_ref) = self.client_ref {
                    tracing::info!("SubAgentChat continuing conversation after tool result for request {}", id);
                    
                    // For screenshot tool results, we need to add the image to the next message
                    let mut messages = state.messages.clone();
                    if let Some(tool_name) = &tool_name_opt {
                        if tool_name == "screenshot" {
                            if let Some(image_url) = self.extract_image_from_result(&result) {
                                // Add a user message with the screenshot image for the vision model
                                let user_msg = OpenAIMessage::User {
                                    content: crate::openai_compat::UserContent::Array(vec![
                                        crate::openai_compat::ContentPart::Text { 
                                            text: "Here is the screenshot you requested. Please describe what you see.".to_string() 
                                        },
                                        crate::openai_compat::ContentPart::Image {
                                            image_url: crate::openai_compat::ImageUrl {
                                                url: image_url,
                                                detail: None,
                                            }
                                        }
                                    ]),
                                    name: None,
                                };
                                messages.push(user_msg);
                            }
                        }
                    }
                    
                    let tools = self.build_tools();
                    client_ref.send_message(ClientMessage::Generate {
                        id,
                        messages,
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
                
                // ------------------------------------------------------------------
                // Process tool calls coming from the OpenAI function-calling API
                // ------------------------------------------------------------------
                if !tool_calls.is_empty() {
                    for call in tool_calls {
                        let tool_id = Uuid::new_v4();
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
                } else if self.enable_tool_api && content.is_some() {
                    // When using function-calling API and no tool calls, this is the final response
                    let response_text = content.as_ref().unwrap();
                    if !response_text.trim().is_empty() {
                        tracing::info!("Function-calling API: No tool calls, treating as final response");
                        let completion_id = state.current_request.unwrap_or(id);
                        myself.send_message(ChatMessage::Complete { 
                            id: completion_id, 
                            response: response_text.clone() 
                        })?;
                    }
                }

                // ------------------------------------------------------------------
                // When the function-calling API is disabled we need to manually
                // look for <tool_call> blocks inside the assistant content.
                // ------------------------------------------------------------------
                if !self.enable_tool_api {
                    if let Some(txt) = &content {
                        let (xml_calls, failures) = Self::extract_xml_tool_calls(txt);
                        tracing::info!("XML tool_call extraction found {} calls ({} parse failures)", xml_calls.len(), failures.len());
                        if !failures.is_empty() {
                            let err_msg = failures.join("; ");
                            tracing::error!("Parse failures: {}", err_msg);
                            // propagate error so caller is informed
                            myself.send_message(ChatMessage::Error { id, error: err_msg.clone() })?;
                        }
                        if xml_calls.is_empty() {
                            tracing::warn!("No <tool_call> blocks found in assistant response when tool API disabled");
                            // If we have content but no tool calls, this is the final response
                            if !txt.trim().is_empty() {
                                tracing::info!("Treating response with content but no tool calls as final for request {}", id);
                                // Use the current_request ID, not the AssistantResponse ID
                                let completion_id = state.current_request.unwrap_or(id);
                                myself.send_message(ChatMessage::Complete { 
                                    id: completion_id, 
                                    response: txt.clone() 
                                })?;
                            }
                        }
                        for (tool_name, params) in xml_calls {
                            let tool_id = Uuid::new_v4();
                            if let Some(tool_ref) = self.tool_actors.get(&tool_name) {
                                state.pending_tool_calls.insert(tool_id, (tool_name.clone(), myself.clone()));
                                tool_ref.send_message(ToolMessage::Execute {
                                    id: tool_id,
                                    params,
                                    chat_ref: myself.clone(),
                                })?;
                            } else {
                                tracing::error!("Tool actor not found: {}", tool_name);
                                myself.send_message(ChatMessage::ToolResult {
                                    id: tool_id,
                                    result: format!("Error: Tool '{}' not available", tool_name),
                                })?;
                            }
                        }
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
            ChatMessage::SwitchSession { .. } => {
                tracing::debug!("SubAgentChat ignoring SwitchSession - not applicable to sub-agents");
            }
        }
        
        Ok(())
    }
}

impl SubAgentChatActor {
    pub fn new(config: Config, tool_actors: HashMap<String, ActorRef<ToolMessage>>, enable_tool_api: bool) -> Self {
        Self {
            config,
            client_ref: None,
            tool_actors,
            enable_tool_api,
        }
    }
    
    pub fn with_client_ref(mut self, client_ref: ActorRef<ClientMessage>) -> Self {
        self.client_ref = Some(client_ref);
        self
    }
    
    fn build_tool_catalogue_xml(&self) -> String {
        let mut parts = Vec::new();
        parts.push("<tools>".to_string());
        for tool_name in self.tool_actors.keys() {
            let def = self.create_tool_definition(tool_name);
            let json_def = serde_json::to_string(&def).unwrap_or_default();
            parts.push(format!("  <tool>{}</tool>", json_def));
        }
        parts.push("</tools>".to_string());
        parts.join("\n")
    }

    fn build_tools(&self) -> Vec<Tool> {
        // If this sub-agent is not allowed to use the function-calling API, return an empty list.
        if !self.enable_tool_api {
            return Vec::new();
        }

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
            "screenshot" => (
                "Take a screenshot of the screen or a region",
                serde_json::json!({
                    "type": "object",
                    "properties": {
                        "action": {
                            "type": "string",
                            "enum": ["capture_screen", "capture_window", "capture_region", "capture_interactive"],
                            "description": "Type of screenshot to take"
                        },
                        "display": {
                            "type": "integer",
                            "description": "Display number (for capture_screen)"
                        },
                        "x": {
                            "type": "integer",
                            "description": "X coordinate for region capture"
                        },
                        "y": {
                            "type": "integer", 
                            "description": "Y coordinate for region capture"
                        },
                        "width": {
                            "type": "integer",
                            "description": "Width for region capture"
                        },
                        "height": {
                            "type": "integer",
                            "description": "Height for region capture"
                        }
                    },
                    "required": ["action"]
                })
            ),
            "desktop_control" => (
                "Control mouse and keyboard on the desktop",
                serde_json::json!({
                    "type": "object",
                    "properties": {
                        "action": {
                            "type": "string",
                            "enum": ["mouse_move", "mouse_click", "mouse_drag", "keyboard_type", "keyboard_key", "get_mouse_position", "check_installation"],
                            "description": "Type of desktop control action"
                        },
                        "x": {
                            "type": "integer",
                            "description": "X coordinate for mouse actions"
                        },
                        "y": {
                            "type": "integer",
                            "description": "Y coordinate for mouse actions"
                        },
                        "from_x": {
                            "type": "integer",
                            "description": "Starting X coordinate for drag"
                        },
                        "from_y": {
                            "type": "integer",
                            "description": "Starting Y coordinate for drag"
                        },
                        "to_x": {
                            "type": "integer",
                            "description": "Ending X coordinate for drag"
                        },
                        "to_y": {
                            "type": "integer",
                            "description": "Ending Y coordinate for drag"
                        },
                        "button": {
                            "type": "string",
                            "enum": ["left", "right", "middle"],
                            "description": "Mouse button to use"
                        },
                        "count": {
                            "type": "integer",
                            "description": "Number of clicks"
                        },
                        "text": {
                            "type": "string",
                            "description": "Text to type"
                        },
                        "key": {
                            "type": "string",
                            "description": "Key or key combination to press"
                        },
                        "duration": {
                            "type": "integer",
                            "description": "Duration in milliseconds"
                        },
                        "delay_ms": {
                            "type": "integer",
                            "description": "Delay between keystrokes in milliseconds"
                        }
                    },
                    "required": ["action"]
                })
            ),
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

    /// Extract <tool_call>JSON</tool_call> blocks from the model output.
    /// The JSON should contain a "name" field with the tool name.
    /// Returns (calls, parse_failures)
    fn extract_xml_tool_calls(text: &str) -> (Vec<(String, serde_json::Value)>, Vec<String>) {
        let mut calls = Vec::new();
        let mut failures = Vec::new();
        // Updated regex to match <tool_call>JSON</tool_call> format
        let re = regex::Regex::new(r#"(?s)<tool_call>(.*?)</tool_call>"#).unwrap();
        for cap in re.captures_iter(text) {
            let json_str = cap.get(1).map(|m| m.as_str().trim()).unwrap_or("");
            match serde_json::from_str::<serde_json::Value>(json_str) {
                Ok(json_obj) => {
                    // Extract tool name from the "name" field in JSON
                    if let Some(name) = json_obj.get("name").and_then(|v| v.as_str()) {
                        calls.push((name.to_string(), json_obj));
                    } else {
                        failures.push(format!("Missing 'name' field in tool call JSON: {}", json_str));
                    }
                },
                Err(e) => failures.push(format!("JSON parse error: {} for: {}", e, json_str)),
            }
        }
        (calls, failures)
    }

    /// Extract image data URL from a screenshot tool result
    fn extract_image_from_result(&self, result: &str) -> Option<String> {
        // Parse the JSON result from screenshot tool
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(result) {
            if let Some(image) = json.get("image").and_then(|v| v.as_str()) {
                return Some(image.to_string());
            }
        }
        None
    }

    /// Process screenshot result to remove large base64 data and provide summary
    fn process_screenshot_result(&self, result: &str) -> String {
        // Parse the JSON result from screenshot tool
        if let Ok(mut json) = serde_json::from_str::<serde_json::Value>(result) {
            // Remove the image data to avoid token limit issues
            if let Some(obj) = json.as_object_mut() {
                if obj.contains_key("image") {
                    obj.remove("image");
                    obj.insert("image".to_string(), serde_json::Value::String("<image_data_removed>".to_string()));
                    obj.insert("note".to_string(), serde_json::Value::String("Screenshot captured successfully. Image data has been provided to the vision model separately.".to_string()));
                }
            }
            serde_json::to_string(&json).unwrap_or_else(|_| result.to_string())
        } else {
            result.to_string()
        }
    }
}