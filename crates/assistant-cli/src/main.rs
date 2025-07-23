use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
    println!("Assistant CLI starting...");
    
    // Initialize the core system
    let system = assistant_core::initialize(None).await?;
    
    println!("Assistant initialized with model: {}", system.config.model);
    
    // TODO: Start TUI
    // TODO: Handle user input
    // TODO: Process commands
    
    Ok(())
}