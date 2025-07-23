use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::Path;
use uuid::Uuid;
use crate::config::tool_config::ToolConfig;
use crate::messages::ToolResult;
use super::base::ToolActorTrait;

/// Actor handling all file system operations
pub struct FileSystemActor;

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "operation")]
pub enum FileSystemOperation {
    Ls { path: String },
    ReadFile { path: String },
    WriteFile { path: String, content: String },
    Edit { path: String, old_content: String, new_content: String },
    Glob { pattern: String },
    Grep { pattern: String, path: Option<String> },
    ReadManyFiles { paths: Vec<String> },
}

#[async_trait]
impl ToolActorTrait for FileSystemActor {
    fn name(&self) -> &str {
        "filesystem"
    }
    
    fn description(&self) -> &str {
        "File system operations including ls, read, write, edit, glob, and grep"
    }
    
    async fn validate_params(&self, params: &Value) -> Result<(), String> {
        // Try to deserialize into our operation enum
        serde_json::from_value::<FileSystemOperation>(params.clone())
            .map(|_| ())
            .map_err(|e| format!("Invalid parameters: {}", e))
    }
    
    async fn execute(
        &self,
        id: Uuid,
        params: Value,
        config: &ToolConfig,
    ) -> Result<ToolResult, anyhow::Error> {
        let operation = serde_json::from_value::<FileSystemOperation>(params)?;
        
        match operation {
            FileSystemOperation::Ls { path } => {
                // TODO: Implement ls operation
                Ok(ToolResult {
                    success: true,
                    output: format!("Listing directory: {}", path),
                    llm_content: "Listed directory contents".to_string(),
                    summary: Some("ls completed".to_string()),
                })
            }
            
            FileSystemOperation::ReadFile { path } => {
                // TODO: Implement file reading
                // For now, just return a placeholder
                Ok(ToolResult {
                    success: true,
                    output: format!("Would read file: {}", path),
                    llm_content: "File contents here".to_string(),
                    summary: Some(format!("Read {}", path)),
                })
            }
            
            FileSystemOperation::WriteFile { path, content } => {
                // TODO: Implement file writing
                Ok(ToolResult {
                    success: true,
                    output: format!("Would write {} bytes to {}", content.len(), path),
                    llm_content: "File written successfully".to_string(),
                    summary: Some(format!("Wrote {}", path)),
                })
            }
            
            // TODO: Implement other operations
            _ => Ok(ToolResult {
                success: false,
                output: "Operation not yet implemented".to_string(),
                llm_content: "Operation not implemented".to_string(),
                summary: None,
            })
        }
    }
    
    async fn needs_confirmation(&self, params: &Value) -> bool {
        if let Ok(op) = serde_json::from_value::<FileSystemOperation>(params.clone()) {
            matches!(op, 
                FileSystemOperation::WriteFile { .. } |
                FileSystemOperation::Edit { .. }
            )
        } else {
            false
        }
    }
}

impl FileSystemActor {
    pub fn new() -> Self {
        Self
    }
}