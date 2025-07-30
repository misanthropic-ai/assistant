use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Configuration for an individual tool
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolConfig {
    /// Whether this tool is enabled
    #[serde(default = "default_true")]
    pub enabled: bool,
    
    /// Whether to delegate this tool to a specialized LLM
    #[serde(default)]
    pub delegate: bool,
    
    /// API key for delegated tool
    pub api_key: Option<String>,
    
    /// Base URL for delegated tool
    pub base_url: Option<String>,
    
    /// Model for delegated tool
    pub model: Option<String>,
    
    /// Temperature for delegated tool
    pub temperature: Option<f32>,
    
    /// System prompt for delegated tool
    pub system_prompt: Option<String>,
    
    /// Tool-specific settings
    #[serde(flatten)]
    pub settings: HashMap<String, serde_json::Value>,
}

/// LLM configuration for tool delegation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LLMConfig {
    /// API key for this specific tool's LLM
    pub api_key: Option<String>,
    
    /// Base URL for the LLM API
    pub base_url: Option<String>,
    
    /// Model to use for this tool
    pub model: Option<String>,
    
    /// Temperature for generation
    pub temperature: Option<f32>,
    
    /// Max tokens for generation
    pub max_tokens: Option<u32>,
    
    /// Custom system prompt for delegated execution
    pub system_prompt: Option<String>,
}

fn default_true() -> bool {
    true
}

impl ToolConfig {
    /// Check if this tool should be delegated to a specialized LLM
    pub fn should_delegate(&self) -> bool {
        self.delegate && self.api_key.is_some() && self.model.is_some()
    }
    
    /// Get a setting value
    pub fn get_setting<T: serde::de::DeserializeOwned>(&self, key: &str) -> Option<T> {
        self.settings.get(key)
            .and_then(|v| serde_json::from_value(v.clone()).ok())
    }
}

impl Default for ToolConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            delegate: false,
            api_key: None,
            base_url: None,
            model: None,
            temperature: None,
            system_prompt: None,
            settings: HashMap::new(),
        }
    }
}

// Example configurations for specific tools
impl ToolConfig {
    /// Create a basic tool config
    pub fn basic() -> Self {
        Self::default()
    }
    
    /// Create a delegated tool config
    pub fn delegated(api_key: String, model: String, system_prompt: String) -> Self {
        Self {
            enabled: true,
            delegate: true,
            api_key: Some(api_key),
            base_url: None,
            model: Some(model),
            temperature: None,
            system_prompt: Some(system_prompt),
            settings: HashMap::new(),
        }
    }
}

// TODO: Implement delegation logic
// - Create separate ClientActor for delegated tools
// - Route tool requests through DelegatorActor
// - Handle specialized prompts and contexts
// - Aggregate responses back to main chat