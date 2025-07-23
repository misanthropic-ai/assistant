use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

/// Core message types for actor communication
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ChatMessage {
    /// User input prompt
    UserPrompt { id: Uuid, prompt: String },
    
    /// Streaming token from LLM
    StreamToken { token: String },
    
    /// Tool execution request
    ToolRequest { id: Uuid, call: ToolCall },
    
    /// Tool execution result
    ToolResult { id: Uuid, result: ToolResult },
    
    /// Completion of response
    Complete { id: Uuid, response: String },
    
    /// Error during processing
    Error { id: Uuid, error: String },
}

/// Tool call information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub tool_name: String,
    pub parameters: serde_json::Value,
    pub delegate: bool,
}

/// Tool execution result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    pub success: bool,
    pub output: String,
    pub llm_content: String,
    pub summary: Option<String>,
}

/// Messages for tool actors
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ToolMessage {
    /// Execute tool with parameters
    Execute {
        id: Uuid,
        params: serde_json::Value,
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
    UserMessage { content: String },
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