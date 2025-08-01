use ractor::{Actor, ActorRef, ActorProcessingErr};
use std::collections::VecDeque;
use crate::config::Config;
use crate::messages::{ChatMessage, DisplayContext, UserMessageContent};
use crate::actors::client::ClientMessage;
use crate::messages::DelegatorMessage;
use crate::actors::chat_persistence::ChatPersistenceMessage;
use crate::openai_compat::{ChatMessage as OpenAIMessage, Tool, FunctionDef, UserContent};
use uuid::Uuid;

/// Main chat actor managing conversation flow
pub struct ChatActor {
    config: Config,
    client_ref: Option<ActorRef<ClientMessage>>,
    delegator_ref: Option<ActorRef<DelegatorMessage>>,
    persistence_ref: Option<ActorRef<ChatPersistenceMessage>>,
    session_id: String,
}

/// Chat actor state
pub struct ChatState {
    history: VecDeque<ChatMessage>,
    messages: Vec<OpenAIMessage>,
    max_history: usize,
    current_request: Option<Uuid>,
    current_context: Option<DisplayContext>,
    delegator_ref: Option<ActorRef<DelegatorMessage>>,
    persistence_ref: Option<ActorRef<ChatPersistenceMessage>>,
    display_refs: std::collections::HashMap<DisplayContext, ActorRef<ChatMessage>>,
    session_id: String,
    // Track active tool calls by ID
    active_tool_calls: std::collections::HashMap<Uuid, String>,
}

impl Actor for ChatActor {
    type Msg = ChatMessage;
    type State = ChatState;
    type Arguments = Config;
    
    async fn pre_start(
        &self,
        _myself: ActorRef<Self::Msg>,
        _config: Self::Arguments,
    ) -> Result<Self::State, ActorProcessingErr> {
        tracing::info!("Chat actor starting");
        
        // Initialize with system prompt
        let mut messages = Vec::new();
        messages.push(OpenAIMessage::System {
            content: self.get_system_prompt(),
            name: None,
        });
        
        Ok(ChatState {
            history: VecDeque::new(),
            messages,
            max_history: 100,
            current_request: None,
            current_context: None,
            delegator_ref: self.delegator_ref.clone(),
            persistence_ref: self.persistence_ref.clone(),
            display_refs: std::collections::HashMap::new(),
            session_id: self.session_id.clone(),
            active_tool_calls: std::collections::HashMap::new(),
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
                // Extract text for logging and persistence
                let prompt_text = match &content {
                    UserMessageContent::Text(text) => text.clone(),
                    UserMessageContent::MultiModal { text, .. } => text.clone(),
                };
                
                tracing::info!("Received user prompt: {}", prompt_text);
                state.current_context = Some(context.clone());
                state.history.push_back(ChatMessage::UserPrompt { id, content: content.clone(), context });
                state.current_request = Some(id);
                
                // Persist user prompt (currently just text)
                if let Some(ref persistence_ref) = state.persistence_ref {
                    persistence_ref.send_message(ChatPersistenceMessage::PersistUserPrompt {
                        id,
                        session_id: state.session_id.clone(),
                        prompt: prompt_text,
                    })?;
                }
                
                // Add user message to conversation
                let user_msg = OpenAIMessage::User {
                    content: match content {
                        UserMessageContent::Text(text) => UserContent::Text(text),
                        UserMessageContent::MultiModal { text, images } => {
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
                
                // Build tools list
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
                // Forward to UI (TODO: implement UI actor)
                tracing::debug!("Stream token: {}", token);
            }
            
            ChatMessage::ToolRequest { id, call } => {
                // This is now handled in AssistantResponse
                tracing::warn!("Received legacy ToolRequest message - this should be handled via AssistantResponse");
                state.history.push_back(ChatMessage::ToolRequest { id, call: call.clone() });
            }
            
            ChatMessage::ToolResult { id, result } => {
                tracing::info!("Tool result received for request {}: {}", id, result);
                state.history.push_back(ChatMessage::ToolResult { id, result: result.clone() });
                
                // Get the tool name from our tracking map
                let tool_name = state.active_tool_calls.remove(&id)
                    .unwrap_or_else(|| "unknown_tool".to_string());
                
                // Persist tool result as a user message (following API convention)
                if let Some(ref persistence_ref) = state.persistence_ref {
                    // Format the tool result as a user message
                    let tool_result_content = format!("Tool result from {}: {}", tool_name, result);
                    persistence_ref.send_message(ChatPersistenceMessage::PersistUserPrompt {
                        id,
                        session_id: state.session_id.clone(),
                        prompt: tool_result_content,
                    })?;
                }
                
                // Add tool result to messages
                let tool_msg = OpenAIMessage::Tool {
                    content: result,
                    tool_call_id: id.to_string(),
                };
                state.messages.push(tool_msg);
                
                // Continue conversation
                if let Some(ref client_ref) = self.client_ref {
                    tracing::info!("Continuing conversation after tool result for request {}", id);
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
                tracing::info!("Assistant response for request {}: content={:?}, tool_calls={}", 
                    id, content, tool_calls.len());
                
                // Store in history
                state.history.push_back(ChatMessage::AssistantResponse { 
                    id, 
                    content: content.clone(), 
                    tool_calls: tool_calls.clone() 
                });
                
                // Persist assistant response immediately
                if content.is_some() || !tool_calls.is_empty() {
                    if let Some(ref persistence_ref) = state.persistence_ref {
                        persistence_ref.send_message(ChatPersistenceMessage::PersistAssistantResponse {
                            id,
                            session_id: state.session_id.clone(),
                            response: content.clone().unwrap_or_default(),
                        })?;
                    }
                    
                    // Build the OpenAI assistant message with tool calls
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
                
                // Now process any tool calls
                for call in tool_calls {
                    let tool_id = Uuid::new_v4();
                    
                    // Track the tool call
                    state.active_tool_calls.insert(tool_id, call.tool_name.clone());
                    
                    // Send tool request to delegator
                    if let Some(ref delegator_ref) = state.delegator_ref {
                        delegator_ref.send_message(DelegatorMessage::RouteToolCall {
                            id: tool_id,
                            call,
                            chat_ref: myself.clone(),
                        })?;
                    }
                }
            }
            
            ChatMessage::Complete { id, response } => {
                tracing::info!("Response complete");
                state.history.push_back(ChatMessage::Complete { id, response: response.clone() });
                
                // No longer persist here - persistence happens in AssistantResponse
                
                // Clear current request
                state.current_request = None;
                
                // Trim history if needed
                while state.history.len() > state.max_history {
                    state.history.pop_front();
                }
            }
            
            ChatMessage::Error { id, error } => {
                tracing::error!("Error in chat: {}", error);
                state.history.push_back(ChatMessage::Error { id, error: error.clone() });
                state.current_request = None;
                // TODO: Handle error - notify UI
            }
            
            ChatMessage::SetDelegatorRef(delegator_ref) => {
                tracing::debug!("Setting delegator actor reference");
                state.delegator_ref = Some(delegator_ref);
            }
            
            ChatMessage::SetPersistenceRef(persistence_ref) => {
                tracing::debug!("Setting persistence actor reference");
                state.persistence_ref = Some(persistence_ref);
            }
            
            ChatMessage::RegisterDisplay { context, display_ref } => {
                tracing::debug!("Registering display actor for context: {:?}", context);
                state.display_refs.insert(context, display_ref);
            }
        }
        
        Ok(())
    }
}

impl ChatActor {
    pub fn new(config: Config, session_id: String) -> Self {
        Self {
            config,
            client_ref: None,
            delegator_ref: None,
            persistence_ref: None,
            session_id,
        }
    }
    
    pub fn with_client_ref(mut self, client_ref: ActorRef<ClientMessage>) -> Self {
        self.client_ref = Some(client_ref);
        self
    }
    
    pub fn with_delegator_ref(mut self, delegator_ref: ActorRef<DelegatorMessage>) -> Self {
        self.delegator_ref = Some(delegator_ref);
        self
    }
    
    pub fn with_persistence_ref(mut self, persistence_ref: ActorRef<ChatPersistenceMessage>) -> Self {
        self.persistence_ref = Some(persistence_ref);
        self
    }
    
    fn get_system_prompt(&self) -> String {
        r#"You are a helpful AI assistant with access to various tools. 

IMPORTANT: Memory Management
- You have a memory tool that allows you to store and retrieve information persistently across conversations
- Use the memory tool to:
  - Store important facts, concepts, or information the user shares with you
  - Search your memory when asked about topics you might have stored
  - Retrieve specific memories when needed
  - Keep track of user preferences or important context

Memory Tool Guidelines:
1. When a user shares important information, proactively store it in memory with appropriate metadata
2. When asked about something, ALWAYS search your memory first before responding from general knowledge
3. Use semantic search to find related concepts even if keywords don't match exactly
4. Store memories with descriptive keys and rich metadata for better organization
5. Periodically check memory stats to understand what information you have stored

Memory Management Operations:
- CREATE: Use 'store' for auto-generated keys or 'store_with_key' for specific keys
- READ: Use 'retrieve' for exact key lookup or 'search' for finding related memories
- UPDATE: Use 'update' to modify existing memories (content and/or metadata)
- DELETE: Use 'delete' to remove specific memories or 'clear' to remove all

When to use each operation:
- store_with_key: Creating new memories or completely replacing existing ones
- update: Modifying parts of existing memories while preserving other data
- delete: Removing outdated or incorrect information
- merge_metadata: When updating, set to true to add new metadata fields without losing existing ones

Examples:
- User: "My favorite programming language is Python" → Store with key "user_favorite_language"
- User: "Actually, I prefer Rust now" → Update the existing memory
- User: "Forget what I said about X" → Delete the specific memory
- User: "What do you know about X?" → Search memory for X before answering

Always be transparent about using the memory tool - let users know when you're storing, updating, or retrieving information."#.to_string()
    }
    
    fn build_tools(&self) -> Vec<Tool> {
        let mut tools = Vec::new();
        
        // Define tools based on config
        let tool_names = [
            "read", "edit", "write", "ls", "glob", "grep",
            "bash", "web_search", "web_fetch", "todo", "memory", "knowledge_agent",
            "screenshot", "desktop_control", "computer_use"
        ];
        
        for tool_name in &tool_names {
            if !self.config.tools.exclude.contains(&tool_name.to_string()) {
                // Get tool config or use defaults
                let tool_config = self.config.tools.configs.get(*tool_name);
                let enabled = tool_config.map(|tc| tc.enabled).unwrap_or(true);
                
                if enabled {
                    let tool = self.create_tool_definition(tool_name);
                    tools.push(tool);
                }
            }
        }
        
        tools
    }
    
    fn create_tool_definition(&self, tool_name: &str) -> Tool {
        // Create tool definitions matching qwen-code tools
        let (description, parameters) = match tool_name {
            "read" => (
                "Read the contents of a file",
                serde_json::json!({
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "The path to the file to read"
                        }
                    },
                    "required": ["path"]
                })
            ),
            "edit" => (
                "Edit a file by replacing content",
                serde_json::json!({
                    "type": "object",
                    "properties": {
                        "file_path": {
                            "type": "string",
                            "description": "The path to the file to edit"
                        },
                        "old_string": {
                            "type": "string",
                            "description": "The exact string to replace"
                        },
                        "new_string": {
                            "type": "string",
                            "description": "The string to replace it with"
                        }
                    },
                    "required": ["file_path", "old_string", "new_string"]
                })
            ),
            "write" => (
                "Write content to a file",
                serde_json::json!({
                    "type": "object",
                    "properties": {
                        "file_path": {
                            "type": "string",
                            "description": "The path to the file to write"
                        },
                        "content": {
                            "type": "string",
                            "description": "The content to write to the file"
                        }
                    },
                    "required": ["file_path", "content"]
                })
            ),
            "ls" => (
                "List files in a directory",
                serde_json::json!({
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "The directory path to list"
                        }
                    },
                    "required": ["path"]
                })
            ),
            "glob" => (
                "Search for files matching a pattern",
                serde_json::json!({
                    "type": "object",
                    "properties": {
                        "pattern": {
                            "type": "string",
                            "description": "The glob pattern to match files"
                        },
                        "path": {
                            "type": "string",
                            "description": "The base directory to search in"
                        }
                    },
                    "required": ["pattern"]
                })
            ),
            "grep" => (
                "Search file contents using regex",
                serde_json::json!({
                    "type": "object",
                    "properties": {
                        "pattern": {
                            "type": "string",
                            "description": "The regex pattern to search for"
                        },
                        "path": {
                            "type": "string",
                            "description": "The path to search in"
                        }
                    },
                    "required": ["pattern"]
                })
            ),
            "bash" => (
                "Execute a bash command",
                serde_json::json!({
                    "type": "object",
                    "properties": {
                        "command": {
                            "type": "string",
                            "description": "The command to execute"
                        }
                    },
                    "required": ["command"]
                })
            ),
            "web_search" => (
                "Search the web for information. Supports complex natural language queries for comprehensive research. This tool uses a dedicated sub-agent that can perform multiple searches and fetch pages to gather detailed information",
                serde_json::json!({
                    "type": "object",
                    "properties": {
                        "query": {
                            "type": "string",
                            "description": "The search query. Can be a simple search term or a detailed natural language request for comprehensive information gathering. The sub-agent will interpret your request and use multiple searches if needed"
                        }
                    },
                    "required": ["query"]
                })
            ),
            "web_fetch" => (
                "Fetch content from a URL and process it with a prompt",
                serde_json::json!({
                    "type": "object",
                    "properties": {
                        "url": {
                            "type": "string",
                            "description": "The URL to fetch"
                        },
                        "prompt": {
                            "type": "string",
                            "description": "What to extract or look for in the fetched content"
                        }
                    },
                    "required": ["url", "prompt"]
                })
            ),
            "todo" => (
                "Manage todo list with various operations",
                serde_json::json!({
                    "type": "object",
                    "properties": {
                        "operation": {
                            "type": "string",
                            "enum": ["list", "add", "update", "remove", "clear", "stats"],
                            "description": "The operation to perform"
                        },
                        "session_id": {
                            "type": "string",
                            "description": "Session ID for the todo list (defaults to 'default')"
                        },
                        "content": {
                            "type": "string",
                            "description": "Content for add operation"
                        },
                        "priority": {
                            "type": "string",
                            "enum": ["low", "medium", "high"],
                            "description": "Priority for add/update/list operations"
                        },
                        "status": {
                            "type": "string",
                            "enum": ["pending", "in_progress", "completed"],
                            "description": "Status for update/list/clear operations"
                        },
                        "id": {
                            "type": "string",
                            "description": "Todo ID for update/remove operations"
                        }
                    },
                    "required": ["operation"]
                })
            ),
            "memory" => (
                "Store and search information in persistent memory with semantic search",
                serde_json::json!({
                    "type": "object",
                    "properties": {
                        "action": {
                            "type": "string",
                            "enum": ["store", "store_with_key", "retrieve", "search", "list", "update", "delete", "clear", "stats"],
                            "description": "The memory action to perform"
                        },
                        "key": {
                            "type": "string",
                            "description": "Memory key (for store_with_key, retrieve, update, delete)"
                        },
                        "content": {
                            "type": "string",
                            "description": "Content to store or update (for store, store_with_key, update)"
                        },
                        "query": {
                            "type": "string",
                            "description": "Search query (for search)"
                        },
                        "limit": {
                            "type": "integer",
                            "description": "Maximum results to return (for search, default: 10)"
                        },
                        "mode": {
                            "type": "string",
                            "enum": ["hybrid", "semantic", "keyword", "exact"],
                            "description": "Search mode (for search, default: hybrid)"
                        },
                        "metadata": {
                            "type": "object",
                            "description": "Metadata to store or update with memory (optional)"
                        },
                        "merge_metadata": {
                            "type": "boolean",
                            "description": "Merge metadata instead of replacing (for update, default: false)"
                        },
                        "metadata_filter": {
                            "type": "object",
                            "description": "Filter search by metadata (optional)"
                        },
                        "prefix": {
                            "type": "string",
                            "description": "Filter list by key prefix (optional)"
                        },
                        "session_only": {
                            "type": "boolean",
                            "description": "Clear only session memories (for clear, default: false)"
                        }
                    },
                    "required": ["action"]
                })
            ),
            "knowledge_agent" => (
                "Search and synthesize knowledge from memories, chat history, todos, and sessions. This tool uses a dedicated sub-agent that can search across all stored information and provide comprehensive analysis",
                serde_json::json!({
                    "type": "object",
                    "properties": {
                        "action": {
                            "type": "string",
                            "enum": ["search", "get_details", "analyze", "synthesize"],
                            "description": "The action to perform"
                        },
                        "query": {
                            "type": "string",
                            "description": "Search query (for search action)"
                        },
                        "topic": {
                            "type": "string",
                            "description": "Topic to analyze or synthesize (for analyze/synthesize actions)"
                        },
                        "source": {
                            "type": "string",
                            "enum": ["memory", "chat_history", "todo", "session", "all"],
                            "description": "Knowledge source (for get_details action)"
                        },
                        "id": {
                            "type": "string",
                            "description": "Item ID (for get_details action)"
                        },
                        "limit": {
                            "type": "integer",
                            "description": "Maximum results to return (for search, default: 20)"
                        },
                        "source_filter": {
                            "type": "array",
                            "items": {
                                "type": "string",
                                "enum": ["memory", "chat_history", "todo", "session"]
                            },
                            "description": "Filter by specific sources (for search)"
                        },
                        "depth": {
                            "type": "string",
                            "enum": ["quick", "standard", "deep"],
                            "description": "Analysis depth (for analyze action, default: standard)"
                        },
                        "include_examples": {
                            "type": "boolean",
                            "description": "Include examples in synthesis (for synthesize action)"
                        }
                    },
                    "required": ["action"]
                })
            ),
            "screenshot" => (
                "Take a screenshot on macOS. Returns a base64 data URL of the screenshot image",
                serde_json::json!({
                    "type": "object",
                    "properties": {
                        "mode": {
                            "type": "string",
                            "enum": ["screen", "window", "region", "interactive"],
                            "description": "Screenshot mode (default: screen)"
                        },
                        "window_id": {
                            "type": "string",
                            "description": "Window ID for window mode (use 'list_windows' first)"
                        },
                        "x": {
                            "type": "integer",
                            "description": "X coordinate for region mode"
                        },
                        "y": {
                            "type": "integer",
                            "description": "Y coordinate for region mode"
                        },
                        "width": {
                            "type": "integer",
                            "description": "Width for region mode"
                        },
                        "height": {
                            "type": "integer",
                            "description": "Height for region mode"
                        },
                        "delay": {
                            "type": "integer",
                            "description": "Delay in seconds before taking screenshot"
                        },
                        "list_windows": {
                            "type": "boolean",
                            "description": "List available windows instead of taking screenshot"
                        }
                    },
                    "required": []
                })
            ),
            "desktop_control" => (
                "Control mouse and keyboard on macOS using cliclick",
                serde_json::json!({
                    "type": "object",
                    "properties": {
                        "action": {
                            "type": "string",
                            "enum": ["mouse_move", "mouse_click", "mouse_drag", "keyboard_type", "keyboard_key", "get_mouse_position", "check_installation"],
                            "description": "The desktop control action to perform"
                        },
                        "x": {
                            "type": "integer",
                            "description": "X coordinate (for mouse actions)"
                        },
                        "y": {
                            "type": "integer",
                            "description": "Y coordinate (for mouse actions)"
                        },
                        "from_x": {
                            "type": "integer",
                            "description": "Starting X coordinate (for mouse_drag)"
                        },
                        "from_y": {
                            "type": "integer",
                            "description": "Starting Y coordinate (for mouse_drag)"
                        },
                        "to_x": {
                            "type": "integer",
                            "description": "Ending X coordinate (for mouse_drag)"
                        },
                        "to_y": {
                            "type": "integer",
                            "description": "Ending Y coordinate (for mouse_drag)"
                        },
                        "button": {
                            "type": "string",
                            "enum": ["left", "right", "middle"],
                            "description": "Mouse button (default: left)"
                        },
                        "count": {
                            "type": "integer",
                            "description": "Click count for mouse_click (1=single, 2=double, 3=triple)"
                        },
                        "duration": {
                            "type": "integer",
                            "description": "Duration in milliseconds for smooth mouse movement"
                        },
                        "text": {
                            "type": "string",
                            "description": "Text to type (for keyboard_type)"
                        },
                        "key": {
                            "type": "string",
                            "description": "Key or key combination to press (e.g., 'cmd+c', 'escape', 'return')"
                        },
                        "delay_ms": {
                            "type": "integer",
                            "description": "Delay between keystrokes in milliseconds"
                        }
                    },
                    "required": ["action"]
                })
            ),
            "computer_use" => (
                "Visual desktop automation agent. This tool uses a dedicated sub-agent with vision capabilities to interact with the desktop through screenshots and control actions",
                serde_json::json!({
                    "type": "object",
                    "properties": {
                        "action": {
                            "type": "string",
                            "enum": ["describe_screen", "navigate_to", "perform_task", "type_text", "read_text", "wait_and_observe"],
                            "description": "The computer use action to perform"
                        },
                        "description": {
                            "type": "string",
                            "description": "Natural language description of what to click/find (for navigate_to)"
                        },
                        "task": {
                            "type": "string",
                            "description": "Natural language description of the task to perform"
                        },
                        "text": {
                            "type": "string",
                            "description": "Text to type in the current focused element"
                        },
                        "region": {
                            "type": "object",
                            "properties": {
                                "x": {"type": "integer"},
                                "y": {"type": "integer"},
                                "width": {"type": "integer"},
                                "height": {"type": "integer"}
                            },
                            "description": "Screen region for describe_screen or read_text actions"
                        },
                        "duration_ms": {
                            "type": "integer",
                            "description": "Duration to wait in milliseconds (for wait_and_observe)"
                        }
                    },
                    "required": ["action"]
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