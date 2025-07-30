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
    }
    
    Ok(())
}