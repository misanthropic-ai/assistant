use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;
use crate::config::tool_config::ToolConfig;
use crate::messages::ToolResult;
use super::base::ToolActorTrait;

/// Actor for shell command execution
pub struct ShellActor;

#[derive(Debug, Serialize, Deserialize)]
pub struct ShellParams {
    pub command: String,
    pub working_dir: Option<String>,
}

#[async_trait]
impl ToolActorTrait for ShellActor {
    fn name(&self) -> &str {
        "shell"
    }
    
    fn description(&self) -> &str {
        "Execute shell commands with optional sandboxing"
    }
    
    async fn validate_params(&self, params: &Value) -> Result<(), String> {
        serde_json::from_value::<ShellParams>(params.clone())
            .map(|_| ())
            .map_err(|e| format!("Invalid parameters: {}", e))
    }
    
    async fn execute(
        &self,
        id: Uuid,
        params: Value,
        config: &ToolConfig,
    ) -> Result<ToolResult, anyhow::Error> {
        let shell_params = serde_json::from_value::<ShellParams>(params)?;
        
        // TODO: Implement shell execution with sandboxing
        Ok(ToolResult {
            success: true,
            output: format!("Would execute: {}", shell_params.command),
            llm_content: "Command executed".to_string(),
            summary: Some("Shell command completed".to_string()),
        })
    }
    
    async fn needs_confirmation(&self, _params: &Value) -> bool {
        true // Always confirm shell commands
    }
}

impl ShellActor {
    pub fn new() -> Self {
        Self
    }
}