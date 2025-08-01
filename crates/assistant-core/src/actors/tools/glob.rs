use ractor::{Actor, ActorRef, ActorProcessingErr};
use serde::{Deserialize, Serialize};
use glob::glob_with;
use glob::MatchOptions;
use crate::config::Config;
use crate::messages::{ToolMessage, ChatMessage};
use crate::utils::path::{resolve_path, validate_path_access};

/// Actor for finding files using glob patterns
pub struct GlobActor {
    #[allow(dead_code)]
    config: Config,
}

/// Glob actor state
pub struct GlobState;

#[derive(Debug, Serialize, Deserialize)]
struct GlobParams {
    pattern: String,
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    case_sensitive: bool,
    #[serde(default = "default_respect_gitignore")]
    respect_git_ignore: bool,
}

fn default_respect_gitignore() -> bool {
    true
}

impl Actor for GlobActor {
    type Msg = ToolMessage;
    type State = GlobState;
    type Arguments = Config;
    
    async fn pre_start(
        &self,
        _myself: ActorRef<Self::Msg>,
        _config: Self::Arguments,
    ) -> Result<Self::State, ActorProcessingErr> {
        tracing::debug!("Glob actor starting");
        Ok(GlobState)
    }
    
    async fn handle(
        &self,
        _myself: ActorRef<Self::Msg>,
        msg: Self::Msg,
        _state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        match msg {
            ToolMessage::Execute { id, params, chat_ref } => {
                tracing::info!("Executing glob tool with params: {:?}", params);
                
                // Parse parameters
                let glob_params: GlobParams = match serde_json::from_value(params) {
                    Ok(p) => p,
                    Err(e) => {
                        chat_ref.send_message(ChatMessage::ToolResult {
                            id,
                            result: format!("Error: Invalid parameters - {}", e),
                        })?;
                        return Ok(());
                    }
                };
                
                // Execute glob operation
                let result = match self.find_files(&glob_params) {
                    Ok(files) => files,
                    Err(e) => format!("Error: {}", e),
                };
                
                // Send result back to chat
                chat_ref.send_message(ChatMessage::ToolResult {
                    id,
                    result,
                })?;
            }
            
            ToolMessage::Cancel { id } => {
                tracing::debug!("Cancelling glob operation {}", id);
                // Glob operations are synchronous, nothing to cancel
            }
            
            ToolMessage::StreamUpdate { .. } => {
                // Glob doesn't stream updates
            }
        }
        
        Ok(())
    }
}

impl GlobActor {
    pub fn new(config: Config) -> Self {
        Self { config }
    }
    
    fn find_files(&self, params: &GlobParams) -> Result<String, String> {
        // Determine base path
        let base_path = match &params.path {
            Some(p) => {
                // Resolve the path (handles both absolute and relative)
                let resolved = match resolve_path(p) {
                    Ok(path) => path,
                    Err(e) => return Err(format!("{}", e)),
                };
                
                // Validate path access
                if let Err(e) = validate_path_access(&resolved) {
                    return Err(format!("{}", e));
                }
                
                if !resolved.is_dir() {
                    return Err(format!("Path is not a directory: {}", p));
                }
                
                resolved.to_string_lossy().to_string()
            }
            None => {
                // Use current working directory
                std::env::current_dir()
                    .map_err(|e| format!("Cannot get current directory: {}", e))?
                    .to_string_lossy()
                    .to_string()
            }
        };
        
        // Build the full pattern
        let full_pattern = if params.pattern.starts_with('/') {
            // Absolute pattern
            params.pattern.clone()
        } else {
            // Relative to base path
            format!("{}/{}", base_path.trim_end_matches('/'), params.pattern)
        };
        
        // Set up match options
        let options = MatchOptions {
            case_sensitive: params.case_sensitive,
            require_literal_separator: false,
            require_literal_leading_dot: false,
        };
        
        // Perform glob search
        let mut matches = Vec::new();
        let glob_result = glob_with(&full_pattern, options)
            .map_err(|e| format!("Invalid glob pattern '{}': {}", params.pattern, e))?;
        
        for entry in glob_result {
            match entry {
                Ok(path) => {
                    // Filter out directories unless explicitly included in pattern
                    if path.is_file() {
                        matches.push(path);
                    }
                }
                Err(e) => {
                    tracing::warn!("Error accessing path during glob: {}", e);
                }
            }
        }
        
        // TODO: Implement gitignore filtering if respect_git_ignore is true
        
        // Sort by modification time (newest first) then by path
        matches.sort_by(|a, b| {
            let a_mtime = std::fs::metadata(a)
                .and_then(|m| m.modified())
                .ok();
            let b_mtime = std::fs::metadata(b)
                .and_then(|m| m.modified())
                .ok();
            
            match (a_mtime, b_mtime) {
                (Some(a_time), Some(b_time)) => b_time.cmp(&a_time),
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (None, None) => a.cmp(b),
            }
        });
        
        if matches.is_empty() {
            Ok(format!(
                "No files found matching pattern '{}' in {}",
                params.pattern, base_path
            ))
        } else {
            let mut result = format!(
                "Found {} files matching '{}' in {}:\n\n",
                matches.len(), params.pattern, base_path
            );
            
            for path in &matches {
                result.push_str(&format!("{}\n", path.display()));
            }
            
            result.push_str(&format!("\nTotal: {} files", matches.len()));
            Ok(result)
        }
    }
}