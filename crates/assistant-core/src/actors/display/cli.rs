use ractor::{Actor, ActorRef, ActorProcessingErr};
use tokio::sync::mpsc;
use crate::messages::ChatMessage;
use super::DisplayActor;

/// CLI display actor that formats output for terminal
pub struct CLIDisplayActor {
    completion_tx: mpsc::UnboundedSender<()>,
}

pub struct CLIDisplayState {
    current_tool: Option<String>,
    has_output: bool,
}

impl Actor for CLIDisplayActor {
    type Msg = ChatMessage;
    type State = CLIDisplayState;
    type Arguments = mpsc::UnboundedSender<()>;
    
    async fn pre_start(
        &self,
        _myself: ActorRef<Self::Msg>,
        _completion_tx: Self::Arguments,
    ) -> Result<Self::State, ActorProcessingErr> {
        Ok(CLIDisplayState {
            current_tool: None,
            has_output: false,
        })
    }
    
    async fn handle(
        &self,
        _myself: ActorRef<Self::Msg>,
        msg: Self::Msg,
        state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        match msg {
            ChatMessage::StreamToken { token } => {
                print!("{}", token);
                use std::io::Write;
                let _ = std::io::stdout().flush();
                state.has_output = true;
            }
            
            ChatMessage::ToolRequest { id: _, call } => {
                if state.has_output {
                    println!(); // Newline after assistant response
                    state.has_output = false;
                }
                println!("\n🔧 Calling tool: {}", call.tool_name);
                println!("   Parameters: {}", serde_json::to_string_pretty(&call.parameters).unwrap_or_default());
                state.current_tool = Some(call.tool_name);
            }
            
            ChatMessage::ToolResult { id: _, result } => {
                if let Some(tool_name) = &state.current_tool {
                    println!("✅ Tool {} completed", tool_name);
                    if !result.trim().is_empty() {
                        // Show truncated result if it's long
                        let display_result = if result.len() > 200 {
                            format!("{}...", &result[..200])
                        } else {
                            result
                        };
                        println!("   Result: {}", display_result);
                    }
                }
                state.current_tool = None;
                println!(); // Blank line before assistant continues
            }
            
            ChatMessage::AssistantResponse { id: _, content, tool_calls } => {
                // Handle any remaining content
                if let Some(text) = content {
                    if !text.is_empty() && !state.has_output {
                        print!("{}", text);
                        use std::io::Write;
                        let _ = std::io::stdout().flush();
                    }
                }
                
                if state.has_output {
                    println!(); // Newline after content
                    state.has_output = false;
                }
                
                // Display tool calls
                for call in &tool_calls {
                    println!("\n🔧 Calling tool: {}", call.tool_name);
                    println!("   Parameters: {}", serde_json::to_string_pretty(&call.parameters).unwrap_or_default());
                    state.current_tool = Some(call.tool_name.clone());
                }
                
                // If no tool calls, signal completion
                if tool_calls.is_empty() {
                    let _ = self.completion_tx.send(());
                }
            }
            
            ChatMessage::Complete { id: _, response: _ } => {
                if state.has_output {
                    println!(); // Final newline
                }
                // Signal completion
                let _ = self.completion_tx.send(());
            }
            
            ChatMessage::Error { id: _, error } => {
                println!("\n❌ Error: {}", error);
                // Signal completion even on error
                let _ = self.completion_tx.send(());
            }
            
            _ => {
                // Ignore other messages
            }
        }
        
        Ok(())
    }
}

impl DisplayActor for CLIDisplayActor {
    fn display_type(&self) -> &'static str {
        "CLI"
    }
}

impl CLIDisplayActor {
    pub fn new(completion_tx: mpsc::UnboundedSender<()>) -> Self {
        Self { completion_tx }
    }
}