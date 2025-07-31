use ractor::{Actor, ActorRef, ActorProcessingErr};
use serde::{Deserialize, Serialize};
use std::path::Path;
use tokio::fs;
use crate::config::Config;
use crate::messages::{ToolMessage, ChatMessage};

pub struct ReadManyFilesActor {
    config: Config,
}

#[derive(Debug, Serialize, Deserialize)]
struct ReadManyFilesParams {
    paths: Vec<String>,
    #[serde(default = "default_max_lines_per_file")]
    max_lines_per_file: usize,
}

fn default_max_lines_per_file() -> usize {
    2000
}

#[derive(Debug)]
struct FileContent {
    path: String,
    content: String,
    error: Option<String>,
}

pub struct ReadManyFilesState;

impl Actor for ReadManyFilesActor {
    type Msg = ToolMessage;
    type State = ReadManyFilesState;
    type Arguments = Config;
    
    async fn pre_start(
        &self,
        _myself: ActorRef<Self::Msg>,
        _config: Self::Arguments,
    ) -> Result<Self::State, ActorProcessingErr> {
        tracing::debug!("ReadManyFiles actor starting");
        Ok(ReadManyFilesState)
    }
    
    async fn handle(
        &self,
        _myself: ActorRef<Self::Msg>,
        msg: Self::Msg,
        _state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        match msg {
            ToolMessage::Execute { id, params, chat_ref } => {
                tracing::info!("ReadManyFiles tool execution with params: {:?}", params);
                
                // Parse parameters
                let read_params: ReadManyFilesParams = match serde_json::from_value(params) {
                    Ok(p) => p,
                    Err(e) => {
                        chat_ref.send_message(ChatMessage::ToolResult {
                            id,
                            result: format!("Error: Invalid parameters - {}", e),
                        })?;
                        return Ok(());
                    }
                };
                
                // Validate parameters
                if read_params.paths.is_empty() {
                    chat_ref.send_message(ChatMessage::ToolResult {
                        id,
                        result: "Error: No file paths provided".to_string(),
                    })?;
                    return Ok(());
                }
                
                // Read all files concurrently
                let mut read_futures = Vec::new();
                for path_str in read_params.paths {
                    let max_lines = read_params.max_lines_per_file;
                    read_futures.push(self.read_file_with_result(path_str, max_lines));
                }
                
                // Wait for all reads to complete
                let results = futures::future::join_all(read_futures).await;
                
                // Format the results
                let formatted_result = self.format_results(results);
                
                // Send result back to chat
                chat_ref.send_message(ChatMessage::ToolResult {
                    id,
                    result: formatted_result,
                })?;
            }
            
            ToolMessage::Cancel { id } => {
                tracing::debug!("Cancelling read many files operation {}", id);
                // File reads are typically quick and not cancellable
            }
            
            ToolMessage::StreamUpdate { .. } => {
                // ReadManyFiles doesn't stream updates
            }
        }
        Ok(())
    }
}

impl ReadManyFilesActor {
    pub fn new(config: Config) -> Self {
        Self { config }
    }
    
    async fn read_file_with_result(&self, path_str: String, max_lines: usize) -> FileContent {
        match self.read_file(&path_str, max_lines).await {
            Ok(content) => FileContent {
                path: path_str,
                content,
                error: None,
            },
            Err(e) => FileContent {
                path: path_str,
                content: String::new(),
                error: Some(e),
            },
        }
    }
    
    async fn read_file(&self, path_str: &str, max_lines: usize) -> Result<String, String> {
        let path = Path::new(path_str);
        
        // Check if file exists
        if !path.exists() {
            return Err(format!("File not found: {}", path_str));
        }
        
        // Check if it's a file
        if !path.is_file() {
            return Err(format!("Not a file: {}", path_str));
        }
        
        // Read the file
        match fs::read_to_string(path).await {
            Ok(content) => {
                // Limit the number of lines
                let lines: Vec<&str> = content.lines().collect();
                let truncated = if lines.len() > max_lines {
                    let truncated_lines: Vec<String> = lines[..max_lines]
                        .iter()
                        .map(|line| line.to_string())
                        .collect();
                    let mut result = truncated_lines.join("\n");
                    result.push_str(&format!("\n... (truncated {} lines)", lines.len() - max_lines));
                    result
                } else {
                    content
                };
                Ok(truncated)
            }
            Err(e) => Err(format!("Error reading file: {}", e)),
        }
    }
    
    fn format_results(&self, results: Vec<FileContent>) -> String {
        let mut output = String::new();
        let total_files = results.len();
        let successful_reads = results.iter().filter(|r| r.error.is_none()).count();
        let failed_reads = total_files - successful_reads;
        
        // Add summary
        output.push_str(&format!(
            "Read {} files ({} successful, {} failed)\n\n",
            total_files, successful_reads, failed_reads
        ));
        
        // Add each file's content
        for (index, file_result) in results.iter().enumerate() {
            output.push_str(&format!("=== File {}/{}: {} ===\n", index + 1, total_files, file_result.path));
            
            match &file_result.error {
                Some(error) => {
                    output.push_str(&format!("ERROR: {}\n", error));
                }
                None => {
                    if file_result.content.is_empty() {
                        output.push_str("(empty file)\n");
                    } else {
                        // Add line numbers
                        let numbered_lines: Vec<String> = file_result.content
                            .lines()
                            .enumerate()
                            .map(|(i, line)| format!("{:>6}\t{}", i + 1, line))
                            .collect();
                        output.push_str(&numbered_lines.join("\n"));
                        output.push('\n');
                    }
                }
            }
            
            output.push_str("\n");
        }
        
        output
    }
}