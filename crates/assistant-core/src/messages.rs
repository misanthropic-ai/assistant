use serde::{Deserialize, Serialize};
use uuid::Uuid;
use ractor::ActorRef;

/// Display context for routing output
#[derive(Debug, Clone, Hash, Eq, PartialEq)]
pub enum DisplayContext {
    /// Command line interface display
    CLI,
    /// Terminal UI display
    TUI,
    /// REST API response
    REST { response_id: Uuid },
    /// Sub-agent internal display
    SubAgent,
    // Future: WebSocket { connection_id: String },
    // Future: Tauri { window_id: String },
}

/// User message content that can be text or multi-modal
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum UserMessageContent {
    Text(String),
    MultiModal {
        text: String,
        images: Vec<String>, // base64 data URLs
    },
}

/// Core message types for actor communication
#[derive(Debug, Clone)]
pub enum ChatMessage {
    /// User input prompt
    UserPrompt { id: Uuid, content: UserMessageContent, context: DisplayContext },
    
    /// Streaming token from LLM
    StreamToken { token: String },
    
    /// Tool execution request
    ToolRequest { id: Uuid, call: ToolCall },
    
    /// Tool execution result
    ToolResult { id: Uuid, result: String },
    
    /// Assistant response (may include tool calls)
    AssistantResponse { 
        id: Uuid, 
        content: Option<String>,
        tool_calls: Vec<ToolCall>,
    },
    
    /// Completion of response
    Complete { id: Uuid, response: String },
    
    /// Error during processing
    Error { id: Uuid, error: String },
    
    /// Set delegator actor reference
    SetDelegatorRef(ActorRef<DelegatorMessage>),
    
    /// Set persistence actor reference  
    SetPersistenceRef(ActorRef<crate::actors::chat_persistence::ChatPersistenceMessage>),
    
    /// Register a display actor for a context
    RegisterDisplay { context: DisplayContext, display_ref: ActorRef<ChatMessage> },
    
    /// Switch to a different conversation session
    SwitchSession { 
        session_id: String, 
        messages: Vec<crate::openai_compat::ChatMessage>,
    },
}

/// Tool call information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub tool_name: String,
    pub parameters: serde_json::Value,
    pub delegate: bool,
}

/// Messages for tool actors
#[derive(Debug, Clone)]
pub enum ToolMessage {
    /// Execute tool with parameters
    Execute {
        id: Uuid,
        params: serde_json::Value,
        chat_ref: ActorRef<ChatMessage>,
    },
    
    /// Cancel ongoing execution
    Cancel { id: Uuid },
    
    /// Stream partial output
    StreamUpdate { id: Uuid, output: String },
}

/// UI display messages
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum UIMessage {
    /// Display content
    Display(DisplayContent),
    
    /// Request user input
    RequestInput { prompt: String },
    
    /// Show confirmation dialog
    ShowConfirmation(ConfirmationRequest),
    
    /// Update statistics
    UpdateStats(Stats),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DisplayContent {
    UserMessage { content: UserMessageContent },
    AssistantMessage { content: String },
    ToolExecution { tool: String, status: String },
    Error { message: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfirmationRequest {
    pub id: Uuid,
    pub tool_name: String,
    pub description: String,
    pub confirm_type: ConfirmationType,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ConfirmationType {
    Execute { command: String },
    Edit { file: String, diff: String },
    WebAccess { url: String },
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Stats {
    pub tokens_used: usize,
    pub tools_executed: usize,
    pub session_duration: u64,
}

/// Delegator messages for tool routing
#[derive(Debug, Clone)]
pub enum DelegatorMessage {
    /// Route a tool call to appropriate actor
    RouteToolCall {
        id: Uuid,
        call: ToolCall,
        chat_ref: ActorRef<ChatMessage>,
    },
    
    /// Register a tool actor
    RegisterTool {
        name: String,
        actor_ref: ActorRef<ToolMessage>,
    },
    
    /// Response from a sub-agent
    SubAgentResponse {
        id: Uuid,
        result: String,
    },
}

/// Supervisor messages for actor lifecycle
#[derive(Debug, Clone)]
pub enum SupervisorMessage {
    /// Start a new session
    StartSession { session_id: Uuid },
    
    /// End current session
    EndSession { session_id: Uuid },
    
    /// Reload configuration
    ReloadConfig,
    
    /// Get system status
    GetStatus,
}