use ractor::{Actor, ActorRef, ActorProcessingErr};
use serde::{Deserialize, Serialize};
use std::fs;
use crate::config::Config;
use crate::messages::{ToolMessage, ChatMessage};
use crate::utils::path::{resolve_path, validate_path_access};
use chrono::{DateTime, Local};

/// Actor for listing directory contents
pub struct LsActor {
    config: Config,
}

pub struct LsState;

#[derive(Debug, Serialize, Deserialize)]
struct LsParams {
    path: String,
    #[serde(default)]
    ignore: Option<Vec<String>>,
    #[serde(default = "default_respect_gitignore")]
    respect_git_ignore: bool,
}

fn default_respect_gitignore() -> bool {
    true
}

#[derive(Debug, Serialize)]
struct FileEntry {
    name: String,
    is_directory: bool,
    size: u64,
    modified_time: String,
}

impl Actor for LsActor {
    type Msg = ToolMessage;
    type State = LsState;
    type Arguments = Config;
    
    async fn pre_start(
        &self,
        _myself: ActorRef<Self::Msg>,
        _config: Self::Arguments,
    ) -> Result<Self::State, ActorProcessingErr> {
        Ok(LsState)
    }
    
    async fn handle(
        &self,
        _myself: ActorRef<Self::Msg>,
        msg: Self::Msg,
        _state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        match msg {
            ToolMessage::Execute { id, params, chat_ref } => {
                tracing::info!("Executing ls tool with params: {:?}", params);
                
                // Parse parameters
                let ls_params: LsParams = match serde_json::from_value(params) {
                    Ok(p) => p,
                    Err(e) => {
                        chat_ref.send_message(ChatMessage::ToolResult {
                            id,
                            result: format!("Error: Invalid parameters - {}", e),
                        })?;
                        return Ok(());
                    }
                };
                
                // Resolve path (handle both absolute and relative paths)
                let canonical_path = match resolve_path(&ls_params.path) {
                    Ok(p) => p,
                    Err(e) => {
                        chat_ref.send_message(ChatMessage::ToolResult {
                            id,
                            result: format!("Error: {}", e),
                        })?;
                        return Ok(());
                    }
                };
                
                // Validate path access
                if let Err(e) = validate_path_access(&canonical_path) {
                    chat_ref.send_message(ChatMessage::ToolResult {
                        id,
                        result: format!("Error: {}", e),
                    })?;
                    return Ok(());
                }
                
                // Update ls_params with the canonical path for consistency
                let canonical_path_str = canonical_path.to_string_lossy().to_string();
                let updated_params = LsParams {
                    path: canonical_path_str.clone(),
                    ignore: ls_params.ignore,
                    respect_git_ignore: ls_params.respect_git_ignore,
                };
                
                // Execute ls operation
                let result = match self.list_directory(&updated_params) {
                    Ok(entries) => {
                        if entries.is_empty() {
                            format!("Directory {} is empty", ls_params.path)
                        } else {
                            self.format_entries(&entries, &ls_params.path)
                        }
                    }
                    Err(e) => format!("Error: {}", e),
                };
                
                // Send result back to chat
                chat_ref.send_message(ChatMessage::ToolResult {
                    id,
                    result,
                })?;
            }
            
            ToolMessage::Cancel { id } => {
                tracing::debug!("Cancelling ls operation {}", id);
                // Ls operations are synchronous, nothing to cancel
            }
            
            ToolMessage::StreamUpdate { .. } => {
                // Ls doesn't stream updates
            }
        }
        
        Ok(())
    }
}

impl LsActor {
    pub fn new(config: Config) -> Self {
        Self { config }
    }
    
    fn list_directory(&self, params: &LsParams) -> Result<Vec<FileEntry>, String> {
        // Check if path exists and is a directory
        let metadata = fs::metadata(&params.path)
            .map_err(|e| format!("Cannot access path '{}': {}", params.path, e))?;
            
        if !metadata.is_dir() {
            return Err(format!("Path is not a directory: {}", params.path));
        }
        
        // Read directory entries
        let entries = fs::read_dir(&params.path)
            .map_err(|e| format!("Cannot read directory '{}': {}", params.path, e))?;
        
        let mut file_entries = Vec::new();
        
        for entry in entries {
            let entry = entry.map_err(|e| format!("Error reading entry: {}", e))?;
            let file_name = entry.file_name().to_string_lossy().to_string();
            
            // Check if should ignore
            if self.should_ignore(&file_name, &params.ignore) {
                continue;
            }
            
            // TODO: Implement git ignore support
            if params.respect_git_ignore {
                // For now, we don't check gitignore
            }
            
            let metadata = entry.metadata()
                .map_err(|e| format!("Cannot read metadata for '{}': {}", file_name, e))?;
            
            let modified: DateTime<Local> = metadata.modified()
                .map_err(|e| format!("Cannot read modified time: {}", e))?
                .into();
                
            file_entries.push(FileEntry {
                name: file_name,
                is_directory: metadata.is_dir(),
                size: if metadata.is_dir() { 0 } else { metadata.len() },
                modified_time: modified.format("%Y-%m-%d %H:%M:%S").to_string(),
            });
        }
        
        // Sort entries: directories first, then by name
        file_entries.sort_by(|a, b| {
            match (a.is_directory, b.is_directory) {
                (true, false) => std::cmp::Ordering::Less,
                (false, true) => std::cmp::Ordering::Greater,
                _ => a.name.cmp(&b.name),
            }
        });
        
        Ok(file_entries)
    }
    
    fn should_ignore(&self, name: &str, ignore_patterns: &Option<Vec<String>>) -> bool {
        if let Some(patterns) = ignore_patterns {
            for pattern in patterns {
                if self.matches_glob(name, pattern) {
                    return true;
                }
            }
        }
        false
    }
    
    fn matches_glob(&self, name: &str, pattern: &str) -> bool {
        // Simple glob matching - just support * for now
        if pattern == "*" {
            return true;
        }
        if pattern.starts_with("*.") {
            let extension = &pattern[2..];
            return name.ends_with(&format!(".{}", extension));
        }
        if pattern.ends_with("*") {
            let prefix = &pattern[..pattern.len()-1];
            return name.starts_with(prefix);
        }
        name == pattern
    }
    
    fn format_entries(&self, entries: &[FileEntry], base_path: &str) -> String {
        let mut output = format!("Directory listing for {}:\n\n", base_path);
        
        // Header
        output.push_str("Type  Size        Modified              Name\n");
        output.push_str("----  ----------  -------------------  ----\n");
        
        for entry in entries {
            let type_char = if entry.is_directory { "d" } else { "f" };
            let size_str = if entry.is_directory {
                "-".to_string()
            } else {
                format_size(entry.size)
            };
            
            output.push_str(&format!(
                "{}     {:>10}  {}  {}\n",
                type_char,
                size_str,
                entry.modified_time,
                entry.name
            ));
        }
        
        output.push_str(&format!("\nTotal: {} items", entries.len()));
        
        output
    }
}

fn format_size(size: u64) -> String {
    const UNITS: &[&str] = &["B", "KB", "MB", "GB", "TB"];
    let mut size = size as f64;
    let mut unit_index = 0;
    
    while size >= 1024.0 && unit_index < UNITS.len() - 1 {
        size /= 1024.0;
        unit_index += 1;
    }
    
    if unit_index == 0 {
        format!("{} {}", size as u64, UNITS[unit_index])
    } else {
        format!("{:.1} {}", size, UNITS[unit_index])
    }
}