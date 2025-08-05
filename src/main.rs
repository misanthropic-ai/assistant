use anyhow::Result;
use assistant_core::Config;
use std::path::Path;

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt::init();
    
    // Parse CLI args to see if any subcommands are provided
    let args: Vec<String> = std::env::args().collect();
    
    // If no args (just the program name) or help flags, launch TUI
    if args.len() == 1 || args.contains(&"--help".to_string()) || args.contains(&"-h".to_string()) {
        launch_tui().await
    } else {
        // Otherwise, use CLI display context
        assistant_cli::run_cli().await
    }
}

async fn launch_tui() -> Result<()> {
    // Load configuration
    let config = match std::env::var("ASSISTANT_CONFIG") {
        Ok(path) => Config::load(Path::new(&path))?,
        Err(_) => Config::load_default().unwrap_or_else(|e| {
            eprintln!("Warning: Could not load config.json: {}", e);
            Config::default()
        }),
    };
    
    // Launch the TUI
    let mut app = assistant_tui::TuiApp::new(config).await?;
    app.run().await?;
    
    Ok(())
}