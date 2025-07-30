use std::collections::HashMap;
use ractor::{Actor, ActorRef};
use crate::config::Config;
use crate::messages::ToolMessage;
use anyhow::Result;

/// Registry for managing available tools
pub struct ToolRegistry {
    config: Config,
}

impl ToolRegistry {
    /// Create a new tool registry
    pub fn new(config: Config) -> Self {
        Self { config }
    }
    
    /// Initialize all enabled tools and return their actor references
    pub async fn initialize_tools(&self) -> Result<HashMap<String, ActorRef<ToolMessage>>> {
        use crate::actors::tools::*;
        
        let mut tool_actors = HashMap::new();
        let exclude_list = &self.config.tools.exclude;
        let tool_config = self.config.clone();
        
        // Helper closure to check if a tool is enabled
        let is_tool_enabled = |name: &str| -> bool {
            if exclude_list.contains(&name.to_string()) {
                tracing::info!("Tool '{}' is excluded by configuration", name);
                return false;
            }
            
            let enabled = self.config.tools.configs
                .get(name)
                .map(|tc| tc.enabled)
                .unwrap_or(true);
                
            if !enabled {
                tracing::info!("Tool '{}' is disabled by configuration", name);
            }
            
            enabled
        };
        
        // Helper macro to register a tool
        macro_rules! register_tool {
            ($name:expr, $actor_type:ty) => {
                if is_tool_enabled($name) {
                    let actor = <$actor_type>::new(tool_config.clone());
                    let (actor_ref, _) = Actor::spawn(
                        Some($name.to_string()), 
                        actor, 
                        tool_config.clone()
                    ).await?;
                    tool_actors.insert($name.to_string(), actor_ref);
                    tracing::info!("Tool '{}' initialized", $name);
                }
            };
        }
        
        // File system tools
        register_tool!("ls", LsActor);
        register_tool!("read", ReadActor);
        register_tool!("write", WriteActor);
        register_tool!("edit", EditActor);
        register_tool!("glob", GlobActor);
        register_tool!("grep", GrepActor);
        register_tool!("read_many_files", ReadManyFilesActor);
        
        // Shell tool
        register_tool!("bash", BashActor);
        
        // Web tools
        register_tool!("web_search", WebSearchActor);
        register_tool!("web_fetch", WebFetchActor);
        
        // Utility tools
        register_tool!("memory", MemoryActor);
        register_tool!("todo", TodoActor);
        
        tracing::info!("Initialized {} tools", tool_actors.len());
        Ok(tool_actors)
    }
    
    /// Get list of all available tool names
    pub fn available_tools() -> Vec<&'static str> {
        vec![
            "ls", "read", "write", "edit", "glob", "grep", "read_many_files",
            "bash", "web_search", "web_fetch", "memory", "todo"
        ]
    }
    
    /// Get list of enabled tools based on current configuration
    pub fn enabled_tools(&self) -> Vec<&str> {
        let exclude_list = &self.config.tools.exclude;
        
        Self::available_tools()
            .into_iter()
            .filter(|name| !exclude_list.contains(&name.to_string()))
            .filter(|name| {
                self.config.tools.configs
                    .get(*name)
                    .map(|tc| tc.enabled)
                    .unwrap_or(true)
            })
            .collect()
    }
    
    /// Get tool descriptions (for help/documentation)
    pub fn tool_descriptions() -> HashMap<&'static str, (&'static str, &'static str)> {
        let mut descriptions = HashMap::new();
        
        // (display_name, description)
        descriptions.insert("ls", ("List Directory", "List files and directories"));
        descriptions.insert("read", ("Read File", "Read the contents of a file"));
        descriptions.insert("write", ("Write File", "Write content to a file"));
        descriptions.insert("edit", ("Edit File", "Edit a file by replacing content"));
        descriptions.insert("glob", ("Glob Search", "Search for files matching a pattern"));
        descriptions.insert("grep", ("Grep Search", "Search file contents using regex"));
        descriptions.insert("read_many_files", ("Read Many Files", "Read multiple files at once"));
        descriptions.insert("bash", ("Shell Command", "Execute a shell command"));
        descriptions.insert("web_search", ("Web Search", "Search the web for information"));
        descriptions.insert("web_fetch", ("Web Fetch", "Fetch content from a URL"));
        descriptions.insert("memory", ("Memory", "Store and retrieve information"));
        descriptions.insert("todo_write", ("Todo List", "Manage a todo list"));
        
        descriptions
    }
}