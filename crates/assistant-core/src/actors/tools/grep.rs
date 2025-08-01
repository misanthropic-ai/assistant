use ractor::{Actor, ActorRef, ActorProcessingErr};
use serde::{Deserialize, Serialize};
use std::path::Path;
use grep::regex::RegexMatcher;
use grep::searcher::{Searcher, SearcherBuilder, Sink, SinkMatch};
use crate::config::Config;
use crate::messages::{ToolMessage, ChatMessage};
use crate::utils::path::{resolve_path, validate_path_access};

/// Actor for searching file contents using regular expressions
pub struct GrepActor {
    #[allow(dead_code)]
    config: Config,
}

/// Grep actor state
pub struct GrepState;

#[derive(Debug, Serialize, Deserialize)]
struct GrepParams {
    pattern: String,
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    glob: Option<String>,
    #[serde(default)]
    r#type: Option<String>,
    #[serde(default = "default_output_mode")]
    output_mode: OutputMode,
    #[serde(rename = "-i", default)]
    case_insensitive: bool,
    #[serde(rename = "-n", default)]
    show_line_numbers: bool,
    #[serde(rename = "-A", default)]
    after_context: usize,
    #[serde(rename = "-B", default)]
    before_context: usize,
    #[serde(rename = "-C", default)]
    context: usize,
    #[serde(default)]
    multiline: bool,
    #[serde(default)]
    head_limit: Option<usize>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
enum OutputMode {
    Content,
    FilesWithMatches,
    Count,
}

fn default_output_mode() -> OutputMode {
    OutputMode::FilesWithMatches
}

impl Actor for GrepActor {
    type Msg = ToolMessage;
    type State = GrepState;
    type Arguments = Config;
    
    async fn pre_start(
        &self,
        _myself: ActorRef<Self::Msg>,
        _config: Self::Arguments,
    ) -> Result<Self::State, ActorProcessingErr> {
        tracing::debug!("Grep actor starting");
        Ok(GrepState)
    }
    
    async fn handle(
        &self,
        _myself: ActorRef<Self::Msg>,
        msg: Self::Msg,
        _state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        match msg {
            ToolMessage::Execute { id, params, chat_ref } => {
                tracing::info!("Executing grep tool with params: {:?}", params);
                
                // Parse parameters
                let grep_params: GrepParams = match serde_json::from_value(params) {
                    Ok(p) => p,
                    Err(e) => {
                        chat_ref.send_message(ChatMessage::ToolResult {
                            id,
                            result: format!("Error: Invalid parameters - {}", e),
                        })?;
                        return Ok(());
                    }
                };
                
                // Execute grep operation
                let result = match self.search_files(&grep_params).await {
                    Ok(output) => output,
                    Err(e) => format!("Error: {}", e),
                };
                
                // Send result back to chat
                chat_ref.send_message(ChatMessage::ToolResult {
                    id,
                    result,
                })?;
            }
            
            ToolMessage::Cancel { id } => {
                tracing::debug!("Cancelling grep operation {}", id);
                // TODO: Implement cancellation for long-running searches
            }
            
            ToolMessage::StreamUpdate { .. } => {
                // Grep doesn't stream updates currently
            }
        }
        
        Ok(())
    }
}

// Sink implementation for collecting search results
struct ResultCollector {
    results: Vec<SearchResult>,
    show_line_numbers: bool,
    max_results: Option<usize>,
}

#[derive(Debug)]
struct SearchResult {
    file_path: String,
    line_number: Option<u64>,
    line_content: String,
    #[allow(dead_code)]
    before_context: Vec<String>,
    #[allow(dead_code)]
    after_context: Vec<String>,
}

impl Sink for ResultCollector {
    type Error = std::io::Error;
    
    fn matched(
        &mut self,
        _searcher: &Searcher,
        mat: &SinkMatch<'_>,
    ) -> Result<bool, Self::Error> {
        // Check if we've reached the limit
        if let Some(max) = self.max_results {
            if self.results.len() >= max {
                return Ok(false); // Stop searching
            }
        }
        
        let line_content = String::from_utf8_lossy(mat.bytes()).to_string();
        let line_number = if self.show_line_numbers {
            mat.line_number()
        } else {
            None
        };
        
        self.results.push(SearchResult {
            file_path: String::new(), // Will be set later
            line_number,
            line_content,
            before_context: Vec::new(),
            after_context: Vec::new(),
        });
        
        Ok(true) // Continue searching
    }
}

impl GrepActor {
    pub fn new(config: Config) -> Self {
        Self { config }
    }
    
    async fn search_files(&self, params: &GrepParams) -> Result<String, String> {
        // Determine search path
        let search_path = match &params.path {
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
        
        // Build regex matcher
        let matcher = if params.case_insensitive {
            let mut builder = grep::regex::RegexMatcherBuilder::new();
            builder.case_insensitive(true);
            builder.build(&params.pattern)
                .map_err(|e| format!("Invalid regex pattern '{}': {}", params.pattern, e))?
        } else {
            RegexMatcher::new(&params.pattern)
                .map_err(|e| format!("Invalid regex pattern '{}': {}", params.pattern, e))?
        };
        
        // Get files to search
        let files_to_search = self.get_files_to_search(&search_path, params)?;
        
        if files_to_search.is_empty() {
            return Ok(format!("No files found to search in {}", search_path));
        }
        
        // Perform search based on output mode
        match params.output_mode {
            OutputMode::Content => self.search_content(files_to_search, matcher, params).await,
            OutputMode::FilesWithMatches => self.search_files_with_matches(files_to_search, matcher, params).await,
            OutputMode::Count => self.search_count(files_to_search, matcher, params).await,
        }
    }
    
    fn get_files_to_search(&self, base_path: &str, params: &GrepParams) -> Result<Vec<String>, String> {
        let path = Path::new(base_path);
        
        if path.is_file() {
            // Single file specified
            return Ok(vec![base_path.to_string()]);
        }
        
        // Directory specified - collect files based on glob/type
        let mut files = Vec::new();
        
        if let Some(glob_pattern) = &params.glob {
            // Use glob pattern
            let pattern = if glob_pattern.starts_with('/') {
                glob_pattern.clone()
            } else {
                format!("{}/{}", base_path.trim_end_matches('/'), glob_pattern)
            };
            
            for entry in glob::glob(&pattern).map_err(|e| format!("Invalid glob pattern: {}", e))? {
                match entry {
                    Ok(path) => {
                        if path.is_file() {
                            files.push(path.to_string_lossy().to_string());
                        }
                    }
                    Err(e) => tracing::warn!("Error accessing path: {}", e),
                }
            }
        } else if let Some(file_type) = &params.r#type {
            // Use file type filter
            files = self.find_files_by_type(base_path, file_type)?;
        } else {
            // Search all files in directory
            files = self.find_all_files(base_path)?;
        }
        
        Ok(files)
    }
    
    fn find_files_by_type(&self, base_path: &str, file_type: &str) -> Result<Vec<String>, String> {
        // Map common file types to extensions
        let extensions: Vec<&str> = match file_type {
            "rust" | "rs" => vec!["rs"],
            "python" | "py" => vec!["py"],
            "javascript" | "js" => vec!["js", "mjs", "cjs"],
            "typescript" | "ts" => vec!["ts", "tsx"],
            "go" => vec!["go"],
            "java" => vec!["java"],
            "cpp" | "c++" => vec!["cpp", "cc", "cxx", "hpp", "h"],
            "c" => vec!["c", "h"],
            "markdown" | "md" => vec!["md", "markdown"],
            "json" => vec!["json"],
            "yaml" | "yml" => vec!["yaml", "yml"],
            "toml" => vec!["toml"],
            _ => return Err(format!("Unknown file type: {}", file_type)),
        };
        
        let mut files = Vec::new();
        for ext in extensions {
            let pattern = format!("{}/**/*.{}", base_path.trim_end_matches('/'), ext);
            for entry in glob::glob(&pattern).map_err(|e| format!("Glob error: {}", e))? {
                if let Ok(path) = entry {
                    if path.is_file() {
                        files.push(path.to_string_lossy().to_string());
                    }
                }
            }
        }
        
        Ok(files)
    }
    
    fn find_all_files(&self, base_path: &str) -> Result<Vec<String>, String> {
        let pattern = format!("{}/**/*", base_path.trim_end_matches('/'));
        let mut files = Vec::new();
        
        for entry in glob::glob(&pattern).map_err(|e| format!("Glob error: {}", e))? {
            if let Ok(path) = entry {
                if path.is_file() {
                    // Skip binary files and common non-text files
                    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
                    if !matches!(ext, "exe" | "dll" | "so" | "dylib" | "pdf" | "jpg" | "jpeg" | "png" | "gif" | "zip" | "tar" | "gz") {
                        files.push(path.to_string_lossy().to_string());
                    }
                }
            }
        }
        
        Ok(files)
    }
    
    async fn search_content(
        &self,
        files: Vec<String>,
        matcher: RegexMatcher,
        params: &GrepParams,
    ) -> Result<String, String> {
        let mut all_results = Vec::new();
        let mut searcher_builder = SearcherBuilder::new();
        
        if params.multiline {
            searcher_builder.multi_line(true);
        }
        
        // Handle context lines
        let before = if params.context > 0 { params.context } else { params.before_context };
        let after = if params.context > 0 { params.context } else { params.after_context };
        
        if before > 0 {
            searcher_builder.before_context(before);
        }
        if after > 0 {
            searcher_builder.after_context(after);
        }
        
        let mut searcher = searcher_builder.build();
        
        for file_path in files {
            let mut collector = ResultCollector {
                results: Vec::new(),
                show_line_numbers: params.show_line_numbers,
                max_results: params.head_limit.map(|limit| limit.saturating_sub(all_results.len())),
            };
            
            match searcher.search_path(&matcher, &file_path, &mut collector) {
                Ok(_) => {
                    for mut result in collector.results {
                        result.file_path = file_path.clone();
                        all_results.push(result);
                        
                        if let Some(limit) = params.head_limit {
                            if all_results.len() >= limit {
                                break;
                            }
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!("Error searching file {}: {}", file_path, e);
                }
            }
            
            if let Some(limit) = params.head_limit {
                if all_results.len() >= limit {
                    break;
                }
            }
        }
        
        if all_results.is_empty() {
            return Ok(format!("No matches found for pattern '{}'", params.pattern));
        }
        
        // Format results
        let mut output = String::new();
        output.push_str(&format!("Found matches for pattern '{}':\n\n", params.pattern));
        
        for result in &all_results {
            if let Some(line_num) = result.line_number {
                output.push_str(&format!("{}:{}:{}\n", result.file_path, line_num, result.line_content.trim_end()));
            } else {
                output.push_str(&format!("{}:{}\n", result.file_path, result.line_content.trim_end()));
            }
        }
        
        if let Some(limit) = params.head_limit {
            if all_results.len() >= limit {
                output.push_str(&format!("\n(Showing first {} matches)", limit));
            }
        }
        
        Ok(output)
    }
    
    async fn search_files_with_matches(
        &self,
        files: Vec<String>,
        matcher: RegexMatcher,
        params: &GrepParams,
    ) -> Result<String, String> {
        let mut matching_files = Vec::new();
        let mut searcher = SearcherBuilder::new().build();
        
        for file_path in files {
            let mut found_match = false;
            let mut sink = grep::searcher::sinks::UTF8(|_, _| {
                found_match = true;
                Ok(false) // Stop after first match
            });
            
            match searcher.search_path(&matcher, &file_path, &mut sink) {
                Ok(_) => {
                    if found_match {
                        matching_files.push(file_path);
                        
                        if let Some(limit) = params.head_limit {
                            if matching_files.len() >= limit {
                                break;
                            }
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!("Error searching file {}: {}", file_path, e);
                }
            }
        }
        
        if matching_files.is_empty() {
            return Ok(format!("No files containing pattern '{}'", params.pattern));
        }
        
        let mut output = String::new();
        output.push_str(&format!("Files containing pattern '{}':\n\n", params.pattern));
        
        for file in &matching_files {
            output.push_str(&format!("{}\n", file));
        }
        
        if let Some(limit) = params.head_limit {
            if matching_files.len() >= limit {
                output.push_str(&format!("\n(Showing first {} files)", limit));
            }
        }
        
        Ok(output)
    }
    
    async fn search_count(
        &self,
        files: Vec<String>,
        matcher: RegexMatcher,
        params: &GrepParams,
    ) -> Result<String, String> {
        let mut file_counts = Vec::new();
        let mut searcher = SearcherBuilder::new().build();
        
        for file_path in files {
            let mut count = 0u64;
            let mut sink = grep::searcher::sinks::UTF8(|_, _| {
                count += 1;
                Ok(true) // Continue counting
            });
            
            match searcher.search_path(&matcher, &file_path, &mut sink) {
                Ok(_) => {
                    if count > 0 {
                        file_counts.push((file_path, count));
                        
                        if let Some(limit) = params.head_limit {
                            if file_counts.len() >= limit {
                                break;
                            }
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!("Error searching file {}: {}", file_path, e);
                }
            }
        }
        
        if file_counts.is_empty() {
            return Ok(format!("No matches found for pattern '{}'", params.pattern));
        }
        
        // Sort by count (descending)
        file_counts.sort_by(|a, b| b.1.cmp(&a.1));
        
        let mut output = String::new();
        output.push_str(&format!("Match counts for pattern '{}':\n\n", params.pattern));
        
        let total_matches: u64 = file_counts.iter().map(|(_, count)| count).sum();
        
        for (file, count) in &file_counts {
            output.push_str(&format!("{}: {}\n", file, count));
        }
        
        output.push_str(&format!("\nTotal: {} matches in {} files", total_matches, file_counts.len()));
        
        if let Some(limit) = params.head_limit {
            if file_counts.len() >= limit {
                output.push_str(&format!(" (showing top {} files)", limit));
            }
        }
        
        Ok(output)
    }
}