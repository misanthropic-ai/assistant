pub mod supervisor;
pub mod chat;
pub mod client;
pub mod delegator;
pub mod tools;
pub mod display;

/// Common error type for actors
#[derive(Debug, thiserror::Error)]
pub enum ActorError {
    #[error("Actor processing error: {0}")]
    Processing(String),
    
    #[error("Configuration error: {0}")]
    Config(String),
    
    #[error("Tool execution error: {0}")]
    ToolExecution(String),
    
    #[error("API error: {0}")]
    Api(String),
    
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}