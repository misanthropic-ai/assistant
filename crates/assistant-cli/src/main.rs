use anyhow::Result;
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
#[command(propagate_version = true)]
struct Cli {
    /// Path to configuration file
    #[arg(short, long, env = "ASSISTANT_CONFIG")]
    config: Option<String>,
    
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Call a tool directly
    Tool {
        /// Name of the tool to call
        name: String,
        
        /// Parameters as JSON string
        params: String,
    },
    
    /// Send a prompt to the chat agent
    Prompt {
        /// The prompt text or JSON message array
        text: String,
        
        /// Maximum iterations for tool calling loop
        #[arg(short, long, default_value = "10")]
        max_iterations: usize,
    },
    
    /// List all available tools
    Tools,
}

mod tool_runner;
mod prompt_runner;
mod actor_init;

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt::init();
    
    let cli = Cli::parse();
    
    match cli.command {
        Commands::Tool { name, params } => {
            // Parse JSON parameters
            let params_json: assistant_core::serde_json::Value = assistant_core::serde_json::from_str(&params)?;
            
            // Run the tool
            let result = tool_runner::run_tool(&name, params_json, cli.config.as_deref()).await?;
            
            // Print the result
            println!("{}", result);
        }
        
        Commands::Prompt { text, max_iterations } => {
            prompt_runner::run_agent_prompt(text, max_iterations, cli.config.as_deref()).await?;
        }
        
        Commands::Tools => {
            use assistant_core::{config::Config, actors::tools::ToolRegistry};
            
            // Load configuration
            let config = match cli.config.as_deref() {
                Some(path) => Config::load(std::path::Path::new(path))?,
                None => Config::load_default().unwrap_or_else(|_| {
                    eprintln!("Warning: Could not load config.json, using defaults");
                    Config::default()
                }),
            };
            
            let registry = ToolRegistry::new(config);
            let all_tools = ToolRegistry::available_tools();
            let enabled_tools = registry.enabled_tools();
            let descriptions = ToolRegistry::tool_descriptions();
            
            println!("Available tools:\n");
            
            for tool in all_tools {
                let enabled = enabled_tools.contains(&tool);
                let status = if enabled { "✓ enabled " } else { "✗ disabled" };
                let description = descriptions.get(tool)
                    .map(|(_, desc)| *desc)
                    .unwrap_or("No description available");
                
                println!("  {} [{}] - {}", 
                    tool.to_string().pad_to_width(20),
                    status,
                    description
                );
            }
            
            println!("\nTo use a tool: assistant tool <name> <params>");
            println!("To enable/disable tools, edit config.json");
        }
    }
    
    Ok(())
}

// Helper trait for padding
trait PadToWidth {
    fn pad_to_width(&self, width: usize) -> String;
}

impl PadToWidth for String {
    fn pad_to_width(&self, width: usize) -> String {
        format!("{:<width$}", self, width = width)
    }
}