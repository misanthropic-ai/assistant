use ractor::{Actor, ActorRef, ActorProcessingErr};
use std::collections::VecDeque;
use crate::config::Config;
use crate::messages::{ChatMessage, DisplayContext};
use crate::actors::client::ClientMessage;
use crate::messages::DelegatorMessage;
use crate::openai_compat::{ChatMessage as OpenAIMessage, Tool, FunctionDef, UserContent};
use uuid::Uuid;

/// Main chat actor managing conversation flow
pub struct ChatActor {
    config: Config,
    client_ref: Option<ActorRef<ClientMessage>>,
    delegator_ref: Option<ActorRef<DelegatorMessage>>,
}

/// Chat actor state
pub struct ChatState {
    history: VecDeque<ChatMessage>,
    messages: Vec<OpenAIMessage>,
    max_history: usize,
    current_request: Option<Uuid>,
    current_context: Option<DisplayContext>,
    delegator_ref: Option<ActorRef<DelegatorMessage>>,
    display_refs: std::collections::HashMap<DisplayContext, ActorRef<ChatMessage>>,
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
        Ok(ChatState {
            history: VecDeque::new(),
            messages: Vec::new(),
            max_history: 100,
            current_request: None,
            current_context: None,
            delegator_ref: self.delegator_ref.clone(),
            display_refs: std::collections::HashMap::new(),
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
            ChatMessage::UserPrompt { id, prompt, context } => {
                tracing::info!("Received user prompt: {}", prompt);
                state.current_context = Some(context.clone());
                state.history.push_back(ChatMessage::UserPrompt { id, prompt: prompt.clone(), context });
                state.current_request = Some(id);
                
                // Add user message to conversation
                let user_msg = OpenAIMessage::User {
                    content: UserContent::Text(prompt),
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
                tracing::info!("Tool request: {}", call.tool_name);
                state.history.push_back(ChatMessage::ToolRequest { id, call: call.clone() });
                
                // Route to delegator
                if let Some(ref delegator_ref) = state.delegator_ref {
                    delegator_ref.send_message(DelegatorMessage::RouteToolCall {
                        id,
                        call,
                        chat_ref: myself.clone(),
                    })?;
                } else {
                    return Err("Delegator actor not set".into());
                }
            }
            
            ChatMessage::ToolResult { id, result } => {
                tracing::info!("Tool result received");
                state.history.push_back(ChatMessage::ToolResult { id, result: result.clone() });
                
                // Add tool result to messages
                let tool_msg = OpenAIMessage::Tool {
                    content: result,
                    tool_call_id: id.to_string(),
                };
                state.messages.push(tool_msg);
                
                // Continue conversation
                if let Some(ref client_ref) = self.client_ref {
                    let tools = self.build_tools();
                    client_ref.send_message(ClientMessage::Generate {
                        id,
                        messages: state.messages.clone(),
                        tools,
                    })?;
                }
            }
            
            ChatMessage::Complete { id, response } => {
                tracing::info!("Response complete");
                state.history.push_back(ChatMessage::Complete { id, response: response.clone() });
                
                // Only add assistant message if there's actual content
                if !response.is_empty() {
                    let assistant_msg = OpenAIMessage::Assistant {
                        content: Some(response),
                        name: None,
                        tool_calls: None,
                    };
                    state.messages.push(assistant_msg);
                }
                
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
            
            ChatMessage::RegisterDisplay { context, display_ref } => {
                tracing::debug!("Registering display actor for context: {:?}", context);
                state.display_refs.insert(context, display_ref);
            }
        }
        
        Ok(())
    }
}

impl ChatActor {
    pub fn new(config: Config) -> Self {
        Self {
            config,
            client_ref: None,
            delegator_ref: None,
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
    
    fn build_tools(&self) -> Vec<Tool> {
        let mut tools = Vec::new();
        
        // Define tools based on config
        let tool_names = [
            "read", "edit", "write", "ls", "glob", "grep",
            "bash", "web_search", "web_fetch", "todo_write", "memory"
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
            "todo_write" => (
                "Update the todo list",
                serde_json::json!({
                    "type": "object",
                    "properties": {
                        "todos": {
                            "type": "array",
                            "items": {
                                "type": "object",
                                "properties": {
                                    "id": {"type": "string"},
                                    "content": {"type": "string"},
                                    "status": {"type": "string", "enum": ["pending", "in_progress", "completed"]},
                                    "priority": {"type": "string", "enum": ["low", "medium", "high"]}
                                },
                                "required": ["id", "content", "status", "priority"]
                            },
                            "description": "The list of todos"
                        }
                    },
                    "required": ["todos"]
                })
            ),
            "memory" => (
                "Store or retrieve from memory",
                serde_json::json!({
                    "type": "object",
                    "properties": {
                        "action": {
                            "type": "string",
                            "enum": ["store", "retrieve", "clear"],
                            "description": "The memory action to perform"
                        },
                        "key": {
                            "type": "string",
                            "description": "The memory key"
                        },
                        "value": {
                            "type": "string",
                            "description": "The value to store (for store action)"
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