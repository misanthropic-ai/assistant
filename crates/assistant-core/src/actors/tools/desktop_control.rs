use ractor::{Actor, ActorRef, ActorProcessingErr};
use serde::{Deserialize, Serialize};
use crate::config::Config;
use crate::messages::{ToolMessage, ChatMessage};
use anyhow::Result;
use std::process::Command;

/// Actor for controlling desktop (mouse and keyboard) on macOS
pub struct DesktopControlActor {
    #[allow(dead_code)]
    config: Config,
}

pub struct DesktopControlState;

/// Desktop control action types
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum DesktopControlAction {
    /// Move mouse to specific coordinates
    MouseMove {
        x: i32,
        y: i32,
        #[serde(default)]
        duration: Option<u32>, // Duration in milliseconds for smooth movement
    },
    
    /// Click at current mouse position or specific coordinates
    MouseClick {
        #[serde(default)]
        x: Option<i32>,
        #[serde(default)]
        y: Option<i32>,
        #[serde(default = "default_button")]
        button: MouseButton,
        #[serde(default = "default_click_count")]
        count: u32, // Single, double, triple click
    },
    
    /// Drag from one position to another
    MouseDrag {
        from_x: i32,
        from_y: i32,
        to_x: i32,
        to_y: i32,
        #[serde(default = "default_button")]
        button: MouseButton,
    },
    
    /// Type text
    KeyboardType {
        text: String,
        #[serde(default)]
        delay_ms: Option<u32>, // Delay between keystrokes
    },
    
    /// Press a key or key combination
    KeyboardKey {
        key: String, // e.g., "cmd+c", "escape", "return"
    },
    
    /// Get current mouse position
    GetMousePosition,
    
    /// Check if cliclick is installed
    CheckInstallation,
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy)]
#[serde(rename_all = "lowercase")]
pub enum MouseButton {
    Left,
    Right,
    Middle,
}

fn default_button() -> MouseButton {
    MouseButton::Left
}

fn default_click_count() -> u32 {
    1
}

impl DesktopControlActor {
    pub fn new(config: Config) -> Self {
        Self { config }
    }
    
    /// Check if cliclick is installed
    fn check_cliclick() -> Result<PathBuf> {
        // Try common locations
        let locations = [
            "/usr/local/bin/cliclick",
            "/opt/homebrew/bin/cliclick",
            "cliclick", // In PATH
        ];
        
        for location in &locations {
            if let Ok(output) = Command::new("which").arg(location).output() {
                if output.status.success() {
                    let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
                    if !path.is_empty() {
                        return Ok(PathBuf::from(path));
                    }
                }
            }
        }
        
        Err(anyhow::anyhow!(
            "cliclick not found. Please install it with: brew install cliclick\n\
             Also ensure Terminal has accessibility permissions in System Preferences > Security & Privacy > Privacy > Accessibility"
        ))
    }
    
    /// Execute desktop control action
    async fn execute_action(&self, action: DesktopControlAction) -> Result<String> {
        if !cfg!(target_os = "macos") {
            // TODO: Add Linux support with xdotool
            return Err(anyhow::anyhow!("Desktop control is only implemented for macOS currently"));
        }
        
        match action {
            DesktopControlAction::CheckInstallation => {
                match Self::check_cliclick() {
                    Ok(path) => Ok(format!("cliclick is installed at: {:?}", path)),
                    Err(e) => Err(e),
                }
            }
            
            DesktopControlAction::GetMousePosition => {
                let cliclick = Self::check_cliclick()?;
                let output = Command::new(cliclick)
                    .arg("p")
                    .output()?;
                
                if output.status.success() {
                    let position = String::from_utf8_lossy(&output.stdout).trim().to_string();
                    Ok(format!("Mouse position: {}", position))
                } else {
                    Err(anyhow::anyhow!("Failed to get mouse position"))
                }
            }
            
            DesktopControlAction::MouseMove { x, y, duration } => {
                let cliclick = Self::check_cliclick()?;
                let mut cmd = Command::new(cliclick);
                
                if let Some(ms) = duration {
                    cmd.arg("-e").arg(ms.to_string());
                }
                
                cmd.arg(format!("m:{},{}", x, y));
                let output = cmd.output()?;
                
                if output.status.success() {
                    Ok(format!("Moved mouse to ({}, {})", x, y))
                } else {
                    Err(anyhow::anyhow!("Failed to move mouse"))
                }
            }
            
            DesktopControlAction::MouseClick { x, y, button, count } => {
                let cliclick = Self::check_cliclick()?;
                let mut cmd = Command::new(cliclick);
                
                // Move to position if specified
                if let (Some(x), Some(y)) = (x, y) {
                    cmd.arg(format!("m:{},{}", x, y));
                }
                
                // Click command
                let click_cmd = match (button, count) {
                    (MouseButton::Left, 1) => "c:.",
                    (MouseButton::Left, 2) => "dc:.",
                    (MouseButton::Left, 3) => "tc:.",
                    (MouseButton::Left, _) => "c:.", // Default to single click for other counts
                    (MouseButton::Right, _) => "rc:.",
                    (MouseButton::Middle, _) => "mc:.",
                };
                
                cmd.arg(click_cmd);
                let output = cmd.output()?;
                
                if output.status.success() {
                    Ok(format!("Clicked {} button {} time(s)", 
                        match button {
                            MouseButton::Left => "left",
                            MouseButton::Right => "right",
                            MouseButton::Middle => "middle",
                        },
                        count
                    ))
                } else {
                    Err(anyhow::anyhow!("Failed to click"))
                }
            }
            
            DesktopControlAction::MouseDrag { from_x, from_y, to_x, to_y, button } => {
                let cliclick = Self::check_cliclick()?;
                let drag_cmd = match button {
                    MouseButton::Left => "dd",
                    MouseButton::Right => "rd",
                    MouseButton::Middle => "md",
                };
                
                let output = Command::new(cliclick)
                    .arg(format!("{}:{},{}", drag_cmd, from_x, from_y))
                    .arg(format!("du:{},{}", to_x, to_y))
                    .output()?;
                
                if output.status.success() {
                    Ok(format!("Dragged from ({}, {}) to ({}, {})", from_x, from_y, to_x, to_y))
                } else {
                    Err(anyhow::anyhow!("Failed to drag"))
                }
            }
            
            DesktopControlAction::KeyboardType { text, delay_ms } => {
                let cliclick = Self::check_cliclick()?;
                let mut cmd = Command::new(cliclick);
                
                if let Some(delay) = delay_ms {
                    cmd.arg("-w").arg(delay.to_string());
                }
                
                // Type text
                cmd.arg(format!("t:{}", text));
                let output = cmd.output()?;
                
                if output.status.success() {
                    Ok(format!("Typed: {}", text))
                } else {
                    Err(anyhow::anyhow!("Failed to type text"))
                }
            }
            
            DesktopControlAction::KeyboardKey { key } => {
                let cliclick = Self::check_cliclick()?;
                
                // Convert common key names to cliclick format
                let cliclick_key = match key.to_lowercase().as_str() {
                    "enter" | "return" => "kp:return".to_string(),
                    "escape" | "esc" => "kp:escape".to_string(),
                    "tab" => "kp:tab".to_string(),
                    "space" => "kp:space".to_string(),
                    "delete" | "backspace" => "kp:delete".to_string(),
                    "up" => "kp:arrow-up".to_string(),
                    "down" => "kp:arrow-down".to_string(),
                    "left" => "kp:arrow-left".to_string(),
                    "right" => "kp:arrow-right".to_string(),
                    // Handle modifier combinations like "cmd+c"
                    _ if key.contains("+") => {
                        format!("kp:{}", key)
                    }
                    _ => format!("kp:{}", key),
                };
                
                let output = Command::new(cliclick)
                    .arg(&cliclick_key)
                    .output()?;
                
                if output.status.success() {
                    Ok(format!("Pressed key: {}", key))
                } else {
                    Err(anyhow::anyhow!("Failed to press key: {}", key))
                }
            }
        }
    }
}

impl Actor for DesktopControlActor {
    type Msg = ToolMessage;
    type State = DesktopControlState;
    type Arguments = Config;
    
    async fn pre_start(
        &self,
        _myself: ActorRef<Self::Msg>,
        _config: Self::Arguments,
    ) -> Result<Self::State, ActorProcessingErr> {
        Ok(DesktopControlState)
    }
    
    async fn handle(
        &self,
        _myself: ActorRef<Self::Msg>,
        msg: Self::Msg,
        _state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        match msg {
            ToolMessage::Execute { id, params, chat_ref } => {
                let action: DesktopControlAction = match serde_json::from_value(params) {
                    Ok(a) => a,
                    Err(e) => {
                        chat_ref.send_message(ChatMessage::ToolResult {
                            id,
                            result: format!("Error parsing desktop control parameters: {}", e),
                        })?;
                        return Ok(());
                    }
                };
                
                let result = match self.execute_action(action).await {
                    Ok(message) => {
                        serde_json::json!({
                            "success": true,
                            "message": message
                        }).to_string()
                    }
                    Err(e) => {
                        serde_json::json!({
                            "success": false,
                            "error": e.to_string()
                        }).to_string()
                    }
                };
                
                chat_ref.send_message(ChatMessage::ToolResult { id, result })?;
            }
            
            ToolMessage::Cancel { .. } => {
                // Desktop control actions are synchronous, can't cancel
            }
            
            ToolMessage::StreamUpdate { .. } => {
                // No streaming for desktop control
            }
        }
        
        Ok(())
    }
}

use std::path::PathBuf;