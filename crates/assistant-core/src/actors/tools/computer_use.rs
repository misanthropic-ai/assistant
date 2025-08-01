use ractor::{Actor, ActorRef, ActorProcessingErr};
use serde::{Deserialize, Serialize};
use serde_json::json;
use crate::config::Config;
use crate::messages::{ToolMessage, ChatMessage};
use anyhow::Result;

/// Actor for computer use - a delegated tool that uses VLM for visual tasks
/// This is primarily a placeholder that gets delegated to a subagent with vision capabilities
pub struct ComputerUseActor {
    #[allow(dead_code)]
    config: Config,
}

pub struct ComputerUseState;

/// Computer use action types
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum ComputerUseAction {
    /// Take a screenshot and describe what's visible
    DescribeScreen {
        #[serde(default)]
        region: Option<ScreenRegion>,
    },
    
    /// Navigate to a specific UI element
    NavigateTo {
        description: String, // Natural language description of what to click
    },
    
    /// Perform a complex UI task
    PerformTask {
        task: String, // Natural language description of the task
    },
    
    /// Type text in the current focused element
    TypeText {
        text: String,
    },
    
    /// Read text from screen
    ReadText {
        #[serde(default)]
        region: Option<ScreenRegion>,
    },
    
    /// Wait and observe changes
    WaitAndObserve {
        duration_ms: u32,
        description: String, // What to look for
    },
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ScreenRegion {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

impl ComputerUseActor {
    pub fn new(config: Config) -> Self {
        Self { config }
    }
}

impl Actor for ComputerUseActor {
    type Msg = ToolMessage;
    type State = ComputerUseState;
    type Arguments = Config;
    
    async fn pre_start(
        &self,
        _myself: ActorRef<Self::Msg>,
        _config: Self::Arguments,
    ) -> Result<Self::State, ActorProcessingErr> {
        Ok(ComputerUseState)
    }
    
    async fn handle(
        &self,
        _myself: ActorRef<Self::Msg>,
        msg: Self::Msg,
        _state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        match msg {
            ToolMessage::Execute { id, params, chat_ref } => {
                // This tool is meant to be delegated to a VLM subagent
                // If called directly, provide instructions
                let action: ComputerUseAction = match serde_json::from_value(params.clone()) {
                    Ok(a) => a,
                    Err(e) => {
                        chat_ref.send_message(ChatMessage::ToolResult {
                            id,
                            result: format!("Error parsing computer use parameters: {}", e),
                        })?;
                        return Ok(());
                    }
                };
                
                // Format the request for the delegated subagent
                let formatted_request = match action {
                    ComputerUseAction::DescribeScreen { ref region } => {
                        if let Some(r) = region {
                            format!("Take a screenshot of the region at ({}, {}) with size {}x{} and describe what you see",
                                r.x, r.y, r.width, r.height)
                        } else {
                            "Take a screenshot of the entire screen and describe what you see".to_string()
                        }
                    }
                    ComputerUseAction::NavigateTo { ref description } => {
                        format!("Take a screenshot, find and click on: {}", description)
                    }
                    ComputerUseAction::PerformTask { ref task } => {
                        format!("Help me with this task: {}", task)
                    }
                    ComputerUseAction::TypeText { ref text } => {
                        format!("Type the following text: {}", text)
                    }
                    ComputerUseAction::ReadText { ref region } => {
                        if let Some(r) = region {
                            format!("Take a screenshot of the region at ({}, {}) with size {}x{} and read any text you see",
                                r.x, r.y, r.width, r.height)
                        } else {
                            "Take a screenshot and read all visible text".to_string()
                        }
                    }
                    ComputerUseAction::WaitAndObserve { duration_ms, ref description } => {
                        format!("Wait {} milliseconds and then check if: {}", duration_ms, description)
                    }
                };
                
                // Return the formatted request
                // The delegator will pass this to the VLM subagent
                let result = json!({
                    "delegated_request": formatted_request,
                    "original_action": serde_json::to_value(&action).unwrap(),
                    "requires_vision": true,
                    "note": "This request will be handled by the computer use subagent with vision capabilities"
                }).to_string();
                
                chat_ref.send_message(ChatMessage::ToolResult { id, result })?;
            }
            
            ToolMessage::Cancel { .. } => {
                // Cancellation handled by subagent
            }
            
            ToolMessage::StreamUpdate { .. } => {
                // Streaming handled by subagent
            }
        }
        
        Ok(())
    }
}