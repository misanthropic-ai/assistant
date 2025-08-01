use assistant_core::config::Config;

/// Create a test configuration with sensible defaults
pub fn test_config() -> Config {
    let mut config = Config::default();
    config.api_key = "test-api-key".to_string();
    config.model = "gpt-4".to_string();
    config.temperature = 0.7;
    config
}