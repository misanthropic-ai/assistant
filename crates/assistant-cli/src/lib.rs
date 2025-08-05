use anyhow::Result;
use clap::{Parser, Subcommand};
use assistant_core::Config;

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
    
    /// Manage TUI sessions
    Tui {
        #[command(subcommand)]
        command: TuiCommands,
    },
    
}

#[derive(Subcommand)]
enum TuiCommands {
    /// List active TUI sessions
    List,
    
    /// Attach to an existing TUI session
    Attach {
        /// Session ID to attach to
        session_id: String,
    },
    
    /// Clean up stale TUI sessions
    Cleanup {
        /// Hours after which a session is considered stale
        #[arg(short, long, default_value = "24")]
        hours: i64,
    },
}

mod tool_runner;
mod prompt_runner;
mod actor_init;

pub async fn run_cli() -> Result<()> {
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
        
        Commands::Tui { command } => {
            use assistant_core::{persistence::{Database, TuiSessionManager}};
            
            // Load configuration
            let config = match cli.config.as_deref() {
                Some(path) => Config::load(std::path::Path::new(path))?,
                None => Config::load_default().unwrap_or_else(|_| {
                    eprintln!("Warning: Could not load config.json, using defaults");
                    Config::default()
                }),
            };
            
            // Get database path
            let db_path = config.session.database_path.clone()
                .unwrap_or_else(|| {
                    dirs::home_dir()
                        .unwrap_or_else(|| std::path::PathBuf::from("/tmp"))
                        .join(".assistant")
                        .join("assistant.db")
                });
            
            // Create database and session manager
            let database = Database::new(&db_path).await?;
            let session_manager = TuiSessionManager::new(database);
            
            match command {
                TuiCommands::List => {
                    let sessions = session_manager.list_active_sessions().await?;
                    
                    if sessions.is_empty() {
                        println!("No active TUI sessions found.");
                    } else {
                        println!("Active TUI sessions:\n");
                        println!("{:<38} {:<30} {:<20} {:<15}", "Session ID", "Command", "Status", "Duration");
                        println!("{}", "-".repeat(103));
                        
                        for session in sessions {
                            let duration = chrono::Utc::now().signed_duration_since(session.created_at);
                            let duration_str = format!("{}h {}m", 
                                duration.num_hours(), 
                                duration.num_minutes() % 60
                            );
                            
                            println!("{:<38} {:<30} {:<20} {:<15}", 
                                session.id,
                                if session.command.len() > 28 { 
                                    format!("{}...", &session.command[..28]) 
                                } else { 
                                    session.command.clone() 
                                },
                                session.status,
                                duration_str
                            );
                        }
                        
                        println!("\nTo attach to a session: assistant tui attach <session-id>");
                    }
                }
                
                TuiCommands::Attach { session_id } => {
                    // Check if session exists
                    match session_manager.get_session(&session_id).await? {
                        Some(session) => {
                            if session.status != "active" {
                                eprintln!("Session {} is not active (status: {})", session_id, session.status);
                                return Ok(());
                            }
                            
                            println!("Attaching to TUI session: {}", session.command);
                            println!("tmux session: {}", session.tmux_session_name);
                            
                            // Use the tui_control tool to capture current state
                            let params = assistant_core::serde_json::json!({
                                "action": "capture_screen",
                                "session_id": session_id
                            });
                            
                            let result = tool_runner::run_tool("tui_control", params, cli.config.as_deref()).await?;
                            println!("\nCurrent screen content:\n{}", result);
                            
                            println!("\nSession attached. Use 'assistant tool tui_control <action>' to interact.");
                        }
                        None => {
                            eprintln!("TUI session '{}' not found.", session_id);
                            eprintln!("Use 'assistant tui list' to see available sessions.");
                        }
                    }
                }
                
                TuiCommands::Cleanup { hours } => {
                    println!("Cleaning up TUI sessions older than {} hours...", hours);
                    
                    let cleaned = session_manager.cleanup_stale_sessions(hours).await?;
                    println!("Marked {} stale sessions as terminated.", cleaned);
                    
                    // Also delete very old terminated sessions (30 days)
                    let deleted = session_manager.delete_old_terminated_sessions(30).await?;
                    if deleted > 0 {
                        println!("Deleted {} old terminated sessions from database.", deleted);
                    }
                }
            }
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