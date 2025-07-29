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
    
    /// Send a prompt to the chat agent (not yet implemented)
    Prompt {
        /// The prompt text
        text: String,
    },
}

mod tool_runner;
mod prompt_runner;

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
        
        Commands::Prompt { text } => {
            prompt_runner::run_prompt(text, cli.config.as_deref()).await?;
        }
    }
    
    Ok(())
}