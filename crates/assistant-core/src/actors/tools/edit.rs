use ractor::{Actor, ActorRef, ActorProcessingErr};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use crate::config::Config;
use crate::messages::{ToolMessage, ChatMessage};
use crate::utils::path::{resolve_path, validate_path_access};
use uuid::Uuid;

/// Actor for editing files by replacing text
pub struct EditActor {
    config: Config,
}

/// Edit actor state
pub struct EditState;

#[derive(Debug, Serialize, Deserialize)]
struct EditParams {
    file_path: String,
    old_string: String,
    new_string: String,
    #[serde(default = "default_expected_replacements")]
    expected_replacements: usize,
}

fn default_expected_replacements() -> usize {
    1
}

impl Actor for EditActor {
    type Msg = ToolMessage;
    type State = EditState;
    type Arguments = Config;
    
    async fn pre_start(
        &self,
        _myself: ActorRef<Self::Msg>,
        _config: Self::Arguments,
    ) -> Result<Self::State, ActorProcessingErr> {
        tracing::debug!("Edit actor starting");
        Ok(EditState)
    }
    
    async fn handle(
        &self,
        _myself: ActorRef<Self::Msg>,
        msg: Self::Msg,
        _state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        match msg {
            ToolMessage::Execute { id, params, chat_ref } => {
                tracing::info!("Executing edit tool with params: {:?}", params);
                
                // Parse parameters
                let edit_params: EditParams = match serde_json::from_value(params) {
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
                let canonical_path = match resolve_path(&edit_params.file_path) {
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
                
                // Update edit_params with the canonical path
                let canonical_params = EditParams {
                    file_path: canonical_path.to_string_lossy().to_string(),
                    old_string: edit_params.old_string,
                    new_string: edit_params.new_string,
                    expected_replacements: edit_params.expected_replacements,
                };
                
                // Execute edit operation
                let result = match self.edit_file(&canonical_params) {
                    Ok(info) => info,
                    Err(e) => format!("Error: {}", e),
                };
                
                // Send result back to chat
                chat_ref.send_message(ChatMessage::ToolResult {
                    id,
                    result,
                })?;
            }
            
            ToolMessage::Cancel { id } => {
                tracing::debug!("Cancelling edit operation {}", id);
                // Edit operations are synchronous, nothing to cancel
            }
            
            ToolMessage::StreamUpdate { .. } => {
                // Edit doesn't stream updates
            }
        }
        
        Ok(())
    }
}

impl EditActor {
    pub fn new(config: Config) -> Self {
        Self { config }
    }
    
    fn edit_file(&self, params: &EditParams) -> Result<String, String> {
        let path = Path::new(&params.file_path);
        
        // Check if it's a new file creation (empty old_string and file doesn't exist)
        let file_exists = path.exists();
        let is_new_file = params.old_string.is_empty() && !file_exists;
        
        if is_new_file {
            // Create new file
            if let Some(parent) = path.parent() {
                if !parent.exists() {
                    fs::create_dir_all(parent)
                        .map_err(|e| format!("Cannot create parent directories for '{}': {}", params.file_path, e))?;
                }
            }
            
            fs::write(&path, &params.new_string)
                .map_err(|e| format!("Cannot create file '{}': {}", params.file_path, e))?;
            
            let lines = params.new_string.lines().count();
            return Ok(format!(
                "Successfully created new file: {}\n\nFile details:\n- Size: {} bytes\n- Lines: {}\n- Path: {}",
                params.file_path,
                params.new_string.len(),
                lines,
                params.file_path
            ));
        }
        
        // Read existing file
        let content = match fs::read_to_string(&path) {
            Ok(content) => content,
            Err(e) => {
                if e.kind() == std::io::ErrorKind::NotFound {
                    return Err(format!("File not found: {}", params.file_path));
                } else {
                    return Err(format!("Cannot read file '{}': {}", params.file_path, e));
                }
            }
        };
        
        // Count occurrences
        let occurrences = content.matches(&params.old_string).count();
        
        if occurrences == 0 {
            return Err(format!(
                "No matches found for the specified old_string in file: {}",
                params.file_path
            ));
        }
        
        if occurrences != params.expected_replacements {
            return Err(format!(
                "Expected {} replacements but found {} occurrences of old_string in file: {}",
                params.expected_replacements, occurrences, params.file_path
            ));
        }
        
        // Perform replacement
        let new_content = content.replace(&params.old_string, &params.new_string);
        
        // Write the file
        fs::write(&path, &new_content)
            .map_err(|e| format!("Cannot write to file '{}': {}", params.file_path, e))?;
        
        // Calculate changes
        let old_lines = content.lines().count();
        let new_lines = new_content.lines().count();
        let line_diff = new_lines as i32 - old_lines as i32;
        let size_diff = new_content.len() as i64 - content.len() as i64;
        
        Ok(format!(
            "Successfully edited file: {}\n\nChanges:\n- Replacements made: {}\n- Lines: {} → {} ({:+})\n- Size: {} → {} bytes ({:+} bytes)\n- Path: {}",
            params.file_path,
            occurrences,
            old_lines,
            new_lines,
            line_diff,
            content.len(),
            new_content.len(),
            size_diff,
            params.file_path
        ))
    }
}