use ractor::{Actor, ActorRef, ActorProcessingErr};
use serde::{Deserialize, Serialize};
use crate::config::Config;
use crate::messages::{ToolMessage, ChatMessage};
use anyhow::Result;
use std::process::Command;
use std::fs;
use base64::{Engine as _, engine::general_purpose::STANDARD};

/// Actor for taking screenshots on macOS
pub struct ScreenshotActor {
    #[allow(dead_code)]
    config: Config,
}

pub struct ScreenshotState;

/// Screenshot action types
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum ScreenshotAction {
    /// Capture the entire screen
    CaptureScreen {
        #[serde(default)]
        display: Option<u32>, // Display number (default: main display)
    },
    
    /// Capture a specific window (interactive)
    CaptureWindow,
    
    /// Capture a specific region
    CaptureRegion {
        x: i32,
        y: i32,
        width: u32,
        height: u32,
    },
    
    /// Capture interactively (user selects region)
    CaptureInteractive,
}

impl ScreenshotActor {
    pub fn new(config: Config) -> Self {
        Self { config }
    }
    
    /// Check if screenshot tools are available
    fn check_availability() -> Result<()> {
        // On macOS, screencapture is built-in
        if !cfg!(target_os = "macos") {
            // TODO: Add Linux support with scrot
            return Err(anyhow::anyhow!("Screenshot tool is only implemented for macOS currently"));
        }
        Ok(())
    }
    
    /// Execute screenshot and return base64 data URL
    async fn take_screenshot(&self, action: ScreenshotAction) -> Result<String> {
        Self::check_availability()?;
        
        // Create temporary file
        let temp_dir = std::env::temp_dir();
        let temp_file = temp_dir.join(format!("screenshot_{}.png", uuid::Uuid::new_v4()));
        
        let mut cmd = Command::new("screencapture");
        
        match action {
            ScreenshotAction::CaptureScreen { display } => {
                // Disable sound
                cmd.arg("-x");
                if let Some(display_num) = display {
                    cmd.arg("-D").arg(display_num.to_string());
                }
                cmd.arg(&temp_file);
            }
            
            ScreenshotAction::CaptureWindow => {
                // Interactive window capture
                cmd.arg("-x") // Disable sound
                   .arg("-w") // Window selection mode
                   .arg(&temp_file);
            }
            
            ScreenshotAction::CaptureRegion { x, y, width, height } => {
                // Capture specific region
                cmd.arg("-x") // Disable sound
                   .arg("-R")
                   .arg(format!("{},{},{},{}", x, y, width, height))
                   .arg(&temp_file);
            }
            
            ScreenshotAction::CaptureInteractive => {
                // Interactive region selection
                cmd.arg("-x") // Disable sound
                   .arg("-i") // Interactive mode
                   .arg(&temp_file);
            }
        }
        
        // Execute screenshot command
        let output = cmd.output()?;
        
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow::anyhow!("Screenshot failed: {}", stderr));
        }
        
        // Check if file was created
        if !temp_file.exists() {
            return Err(anyhow::anyhow!("Screenshot was cancelled or failed"));
        }
        
        // Read file and convert to base64
        let image_data = fs::read(&temp_file)?;
        let base64_data = STANDARD.encode(&image_data);
        
        // Clean up temp file
        let _ = fs::remove_file(&temp_file);
        
        // Return as data URL
        Ok(format!("data:image/png;base64,{}", base64_data))
    }
}

impl Actor for ScreenshotActor {
    type Msg = ToolMessage;
    type State = ScreenshotState;
    type Arguments = Config;
    
    async fn pre_start(
        &self,
        _myself: ActorRef<Self::Msg>,
        _config: Self::Arguments,
    ) -> Result<Self::State, ActorProcessingErr> {
        Ok(ScreenshotState)
    }
    
    async fn handle(
        &self,
        _myself: ActorRef<Self::Msg>,
        msg: Self::Msg,
        _state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        match msg {
            ToolMessage::Execute { id, params, chat_ref } => {
                let action: ScreenshotAction = match serde_json::from_value(params) {
                    Ok(a) => a,
                    Err(e) => {
                        chat_ref.send_message(ChatMessage::ToolResult {
                            id,
                            result: format!("Error parsing screenshot parameters: {}", e),
                        })?;
                        return Ok(());
                    }
                };
                
                let result = match self.take_screenshot(action).await {
                    Ok(data_url) => {
                        // Return the data URL with some metadata
                        serde_json::json!({
                            "success": true,
                            "image": data_url,
                            "format": "png",
                            "encoding": "base64"
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
                // Screenshot is synchronous, can't cancel
            }
            
            ToolMessage::StreamUpdate { .. } => {
                // No streaming for screenshots
            }
        }
        
        Ok(())
    }
}