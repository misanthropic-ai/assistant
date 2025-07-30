pub mod tool_config;

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use anyhow::Result;

use tool_config::ToolConfig;
use crate::persistence::SessionMode;

/// Main configuration structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Primary API key
    pub api_key: String,
    
    /// Base URL for API
    #[serde(default = "default_base_url")]
    pub base_url: String,
    
    /// Model to use
    #[serde(default = "default_model")]
    pub model: String,
    
    /// Temperature for generation
    #[serde(default = "default_temperature")]
    pub temperature: f32,
    
    /// Maximum tokens for generation
    #[serde(default = "default_max_tokens")]
    pub max_tokens: u32,
    
    /// Tool configurations
    #[serde(default)]
    pub tools: ToolsConfig,
    
    /// Telemetry settings
    #[serde(default)]
    pub telemetry: TelemetryConfig,
    
    /// Session configuration
    #[serde(default)]
    pub session: SessionConfig,
    
    /// Embedding configuration
    #[serde(default)]
    pub embeddings: EmbeddingConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ToolsConfig {
    /// Tools to exclude (blacklist)
    #[serde(default)]
    pub exclude: Vec<String>,
    
    /// Individual tool configurations
    #[serde(flatten)]
    pub configs: HashMap<String, ToolConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TelemetryConfig {
    #[serde(default)]
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionConfig {
    /// Session mode
    #[serde(default)]
    pub mode: SessionMode,
    
    /// Explicit session ID (used with SessionMode::Explicit)
    pub session_id: Option<String>,
    
    /// Workspace path for session context
    pub workspace_path: Option<PathBuf>,
    
    /// Database path (defaults to ~/.assistant/assistant.db)
    pub database_path: Option<PathBuf>,
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            mode: SessionMode::default(),
            session_id: None,
            workspace_path: None,
            database_path: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbeddingConfig {
    /// Model configurations by name
    #[serde(default)]
    pub models: HashMap<String, EmbeddingModelConfig>,
    
    /// Default model to use
    #[serde(default = "default_embedding_model")]
    pub default_model: String,
    
    /// Cache size for embeddings
    #[serde(default = "default_cache_size")]
    pub cache_size: usize,
    
    /// Device preference for local models
    #[serde(default)]
    pub device_preference: crate::embeddings::device::DevicePreference,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbeddingModelConfig {
    /// Provider type (openai, local, etc)
    pub provider: String,
    
    /// Model name/identifier
    pub model: String,
    
    /// API key for this model (if needed)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
    
    /// Base URL for this model (if needed)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    
    /// Provider-specific settings
    #[serde(flatten)]
    pub settings: HashMap<String, serde_json::Value>,
}

impl Default for EmbeddingConfig {
    fn default() -> Self {
        let mut models = HashMap::new();
        
        // Add default OpenAI model
        models.insert(
            "openai-small".to_string(),
            EmbeddingModelConfig {
                provider: "openai".to_string(),
                model: "text-embedding-3-small".to_string(),
                api_key: None,
                base_url: None,
                settings: HashMap::new(),
            },
        );
        
        Self {
            models,
            default_model: default_embedding_model(),
            cache_size: default_cache_size(),
            device_preference: crate::embeddings::device::DevicePreference::default(),
        }
    }
}

fn default_cache_size() -> usize {
    1000
}

fn default_embedding_model() -> String {
    "openai-small".to_string()
}

fn default_base_url() -> String {
    "https://api.openai.com/v1".to_string()
}

fn default_model() -> String {
    "gpt-4".to_string()
}

fn default_temperature() -> f32 {
    0.0
}

fn default_max_tokens() -> u32 {
    4096
}

impl Default for Config {
    fn default() -> Self {
        Self {
            api_key: "test-api-key".to_string(),
            base_url: default_base_url(),
            model: default_model(),
            temperature: default_temperature(),
            max_tokens: default_max_tokens(),
            tools: ToolsConfig::default(),
            telemetry: TelemetryConfig::default(),
            session: SessionConfig::default(),
            embeddings: EmbeddingConfig::default(),
        }
    }
}

impl Config {
    /// Load configuration from file
    pub fn load(path: &Path) -> Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let config: Config = serde_json::from_str(&content)?;
        Ok(config)
    }
    
    /// Load from default location (./config.json)
    pub fn load_default() -> Result<Self> {
        Self::load(Path::new("config.json"))
    }
    
    /// Check if a tool is enabled
    pub fn is_tool_enabled(&self, tool_name: &str) -> bool {
        !self.tools.exclude.contains(&tool_name.to_string())
    }
    
    /// Get configuration for a specific tool
    pub fn get_tool_config(&self, tool_name: &str) -> Option<&ToolConfig> {
        self.tools.configs.get(tool_name)
    }
}