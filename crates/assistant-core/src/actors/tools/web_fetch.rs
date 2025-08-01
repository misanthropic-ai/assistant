use ractor::{Actor, ActorRef, ActorProcessingErr};
use serde::{Deserialize, Serialize};
use reqwest::{Client, Url};
use html2text;
use std::time::Duration;
use crate::config::Config;
use crate::messages::{ToolMessage, ChatMessage};

/// Actor for fetching and processing web content
pub struct WebFetchActor {
    #[allow(dead_code)]
    config: Config,
    client: Client,
}

/// WebFetch actor state
pub struct WebFetchState;

#[derive(Debug, Serialize, Deserialize)]
struct WebFetchParams {
    url: String,
    prompt: String,
}

impl Actor for WebFetchActor {
    type Msg = ToolMessage;
    type State = WebFetchState;
    type Arguments = Config;
    
    async fn pre_start(
        &self,
        _myself: ActorRef<Self::Msg>,
        _config: Self::Arguments,
    ) -> Result<Self::State, ActorProcessingErr> {
        tracing::debug!("WebFetch actor starting");
        Ok(WebFetchState)
    }
    
    async fn handle(
        &self,
        _myself: ActorRef<Self::Msg>,
        msg: Self::Msg,
        _state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        match msg {
            ToolMessage::Execute { id, params, chat_ref } => {
                tracing::info!("Executing web fetch with params: {:?}", params);
                
                // Parse parameters
                let fetch_params: WebFetchParams = match serde_json::from_value(params) {
                    Ok(p) => p,
                    Err(e) => {
                        chat_ref.send_message(ChatMessage::ToolResult {
                            id,
                            result: format!("Error: Invalid parameters - {}", e),
                        })?;
                        return Ok(());
                    }
                };
                
                // Execute web fetch
                let result = self.fetch_and_process(&fetch_params).await;
                
                // Send result back to chat
                chat_ref.send_message(ChatMessage::ToolResult {
                    id,
                    result,
                })?;
            }
            
            ToolMessage::Cancel { id } => {
                tracing::debug!("Cancelling web fetch operation {}", id);
                // Web fetch operations are not cancellable once started
            }
            
            ToolMessage::StreamUpdate { .. } => {
                // WebFetch doesn't stream updates
            }
        }
        
        Ok(())
    }
}

impl WebFetchActor {
    pub fn new(config: Config) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(30))
            .user_agent("Mozilla/5.0 (compatible; assistant-core/0.1)")
            .build()
            .unwrap_or_default();
            
        Self { config, client }
    }
    
    async fn fetch_and_process(&self, params: &WebFetchParams) -> String {
        // Validate URL
        let url = match Url::parse(&params.url) {
            Ok(url) => url,
            Err(e) => {
                return format!("Error: Invalid URL '{}' - {}", params.url, e);
            }
        };
        
        // Fetch the content
        let response = match self.client.get(url.clone()).send().await {
            Ok(resp) => resp,
            Err(e) => {
                return format!("Error fetching URL '{}': {}", url, e);
            }
        };
        
        // Check status
        if !response.status().is_success() {
            return format!("Error: HTTP {} when fetching '{}'", response.status(), url);
        }
        
        // Check for redirects
        if let Some(final_url) = response.url().host_str() {
            if let Some(original_host) = url.host_str() {
                if final_url != original_host {
                    return format!(
                        "Redirect detected: The URL redirected to a different host.\n\
                        Original: {}\n\
                        Redirected to: {}\n\n\
                        Please make a new WebFetch request with the redirect URL if you want to fetch its content.",
                        url, response.url()
                    );
                }
            }
        }
        
        // Get content type
        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("text/html")
            .to_string();
        
        // Get the content
        let content = match response.text().await {
            Ok(text) => text,
            Err(e) => {
                return format!("Error reading response from '{}': {}", url, e);
            }
        };
        
        // Process based on content type
        let processed_content = if content_type.contains("text/html") {
            // Convert HTML to markdown
            match html2text::from_read(content.as_bytes(), 80) {
                Ok(markdown) => markdown,
                Err(_) => content,
            }
        } else if content_type.contains("application/json") {
            // Pretty print JSON
            match serde_json::from_str::<serde_json::Value>(&content) {
                Ok(json) => serde_json::to_string_pretty(&json).unwrap_or(content),
                Err(_) => content,
            }
        } else {
            // Return as-is for other content types
            content
        };
        
        // Truncate if too long
        let max_length = 50000;
        let content_length = processed_content.len();
        let truncated = if content_length > max_length {
            format!(
                "{}...\n\n[Content truncated - {} characters omitted]",
                &processed_content[..max_length],
                content_length - max_length
            )
        } else {
            processed_content
        };
        
        // Format the result with the prompt context
        format!(
            "Fetched content from: {}\n\
            Content-Type: {}\n\
            Length: {} characters\n\n\
            Content:\n{}\n\n\
            Analysis based on prompt: \"{}\"\n\n\
            The content above shows the fetched web page. {}",
            url,
            content_type,
            content_length,
            truncated,
            params.prompt,
            self.analyze_content(&truncated, &params.prompt)
        )
    }
    
    fn analyze_content(&self, content: &str, prompt: &str) -> String {
        // Simple analysis based on the prompt
        // In a real implementation, this would use an AI model
        
        let _lower_content = content.to_lowercase();
        let lower_prompt = prompt.to_lowercase();
        
        // Basic keyword matching
        let keywords: Vec<&str> = lower_prompt
            .split_whitespace()
            .filter(|w| w.len() > 3)
            .collect();
        
        let mut relevant_sections = Vec::new();
        
        for line in content.lines() {
            let lower_line = line.to_lowercase();
            if keywords.iter().any(|kw| lower_line.contains(kw)) {
                relevant_sections.push(line);
            }
        }
        
        if relevant_sections.is_empty() {
            format!("No sections directly matching the prompt keywords were found. Please review the full content above.")
        } else {
            format!(
                "Found {} sections potentially relevant to your prompt. Key sections include:\n{}",
                relevant_sections.len(),
                relevant_sections.iter().take(5).map(|s| format!("- {}", s)).collect::<Vec<_>>().join("\n")
            )
        }
    }
}