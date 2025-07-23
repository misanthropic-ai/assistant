use async_trait::async_trait;
use ractor::{Actor, ActorRef, ActorProcessingErr};
use serde_json::Value;
use uuid::Uuid;
use crate::config::tool_config::ToolConfig;
use crate::messages::{ToolMessage, ToolResult};

/// Base trait for all tool actors
#[async_trait]
pub trait ToolActorTrait: Send + Sync + 'static {
    /// Get the tool name
    fn name(&self) -> &str;
    
    /// Get the tool description
    fn description(&self) -> &str;
    
    /// Validate parameters before execution
    async fn validate_params(&self, params: &Value) -> Result<(), String>;
    
    /// Execute the tool with given parameters
    async fn execute(
        &self,
        id: Uuid,
        params: Value,
        config: &ToolConfig,
    ) -> Result<ToolResult, anyhow::Error>;
    
    /// Check if confirmation is needed
    async fn needs_confirmation(&self, params: &Value) -> bool {
        false
    }
    
    /// Get confirmation message
    async fn get_confirmation_message(&self, params: &Value) -> Option<String> {
        None
    }
}

/// Base tool actor implementation
pub struct ToolActor<T: ToolActorTrait> {
    inner: T,
    config: ToolConfig,
}

impl<T: ToolActorTrait> Actor for ToolActor<T> {
    type Msg = ToolMessage;
    type State = ();
    type Arguments = (T, ToolConfig);
    
    async fn pre_start(
        &self,
        _myself: ActorRef<Self::Msg>,
        (_inner, _config): Self::Arguments,
    ) -> Result<Self::State, ActorProcessingErr> {
        let tool_name = self.inner.name();
        
        if self.config.should_delegate() {
            // TODO: Register with DelegatorActor
            // TODO: Create specialized ClientActor
            tracing::info!(
                "Tool '{}' configured for delegation to {}",
                tool_name,
                self.config.llm_config.as_ref()
                    .and_then(|c| c.model.as_ref())
                    .unwrap_or(&"default".to_string())
            );
        }
        
        tracing::info!("Tool actor '{}' started", tool_name);
        Ok(())
    }
    
    async fn handle(
        &self,
        _myself: ActorRef<Self::Msg>,
        msg: Self::Msg,
        _state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        match msg {
            ToolMessage::Execute { id, params } => {
                tracing::debug!("Tool '{}' executing with params: {:?}", self.inner.name(), params);
                
                // Validate parameters
                if let Err(e) = self.inner.validate_params(&params).await {
                    tracing::error!("Parameter validation failed: {}", e);
                    // TODO: Send error result back
                    return Ok(());
                }
                
                // Execute tool
                match self.inner.execute(id, params, &self.config).await {
                    Ok(result) => {
                        tracing::debug!("Tool '{}' execution successful", self.inner.name());
                        // TODO: Send result back
                    }
                    Err(e) => {
                        tracing::error!("Tool '{}' execution failed: {}", self.inner.name(), e);
                        // TODO: Send error result back
                    }
                }
            }
            
            ToolMessage::Cancel { id } => {
                tracing::info!("Cancelling tool execution: {}", id);
                // TODO: Implement cancellation
            }
            
            ToolMessage::StreamUpdate { id, output } => {
                tracing::debug!("Stream update for {}: {}", id, output);
                // TODO: Forward stream updates
            }
        }
        
        Ok(())
    }
}

impl<T: ToolActorTrait> ToolActor<T> {
    pub fn new(inner: T, config: ToolConfig) -> Self {
        Self { inner, config }
    }
}