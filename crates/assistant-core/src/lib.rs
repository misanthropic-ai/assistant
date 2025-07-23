pub mod actors;
pub mod config;
pub mod messages;

// Re-export commonly used types
pub use config::Config;
pub use messages::*;

use anyhow::Result;
use ractor::{Actor, ActorRef};
use tokio::task::JoinHandle;

/// Initialize the assistant core system
pub async fn initialize(config_path: Option<&str>) -> Result<AssistantSystem> {
    // Initialize tracing
    tracing_subscriber::fmt::init();
    
    // Load configuration
    let config = match config_path {
        Some(path) => Config::load(std::path::Path::new(path))?,
        None => Config::load_default()?,
    };
    
    // Start the supervisor actor
    let (supervisor_ref, supervisor_handle) = Actor::spawn(
        Some("supervisor".to_string()),
        actors::supervisor::SupervisorActor::new(config.clone()),
        config.clone(),
    ).await?;
    
    Ok(AssistantSystem {
        config,
        supervisor: supervisor_ref,
        _handle: supervisor_handle,
    })
}

/// Handle to the running assistant system
pub struct AssistantSystem {
    pub config: Config,
    pub supervisor: ActorRef<messages::SupervisorMessage>,
    _handle: JoinHandle<()>,
}