pub mod tool_config;

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use anyhow::Result;

use tool_config::ToolConfig;

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