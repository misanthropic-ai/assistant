use anyhow::{Result, anyhow};
use futures::stream::{Stream, StreamExt};
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE};
use eventsource_stream::Eventsource;
use std::pin::Pin;
use crate::config::Config;
use super::types::*;

/// OpenAI-compatible API client with streaming support
pub struct OpenAICompatClient {
    client: reqwest::Client,
    base_url: String,
    #[allow(dead_code)]
    api_key: String,
}

impl OpenAICompatClient {
    pub fn new(config: &Config) -> Self {
        let mut headers = HeaderMap::new();
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_str(&format!("Bearer {}", config.api_key))
                .expect("Invalid API key format"),
        );
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        
        let client = reqwest::Client::builder()
            .default_headers(headers)
            .timeout(std::time::Duration::from_secs(120))
            .build()
            .expect("Failed to create HTTP client");
        
        Self {
            client,
            base_url: config.base_url.clone(),
            api_key: config.api_key.clone(),
        }
    }
    
    /// Create a streaming chat completion
    pub async fn create_chat_completion_stream(
        &self,
        mut request: ChatCompletionRequest,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<ChatCompletionChunk>> + Send>>> {
        request.stream = true;
        
        let url = format!("{}/chat/completions", self.base_url);
        
        let response = self.client
            .post(&url)
            .json(&request)
            .send()
            .await?;
        
        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await?;
            
            // Try to parse as error response
            if let Ok(error_resp) = serde_json::from_str::<ErrorResponse>(&text) {
                return Err(anyhow!("API error {}: {}", status, error_resp.error.message));
            }
            
            return Err(anyhow!("API error {}: {}", status, text));
        }
        
        // Convert response bytes stream to SSE events
        let event_stream = response.bytes_stream().eventsource();
        
        let stream = async_stream::stream! {
            futures::pin_mut!(event_stream);
            
            while let Some(event) = event_stream.next().await {
                match event {
                    Ok(event) => {
                        // Get the data field
                        let data = event.data;
                        
                        // Skip the [DONE] message
                        if data == "[DONE]" {
                            tracing::debug!("Stream complete");
                            break;
                        }
                        
                        // Skip empty data
                        if data.trim().is_empty() {
                            continue;
                        }
                        
                        // Parse the chunk
                        match serde_json::from_str::<ChatCompletionChunk>(&data) {
                            Ok(chunk) => {
                                yield Ok(chunk);
                            }
                            Err(e) => {
                                tracing::error!("Failed to parse chunk: {} - Data: {}", e, data);
                                yield Err(anyhow!("Failed to parse chunk: {}", e));
                            }
                        }
                    }
                    Err(e) => {
                        tracing::error!("SSE error: {}", e);
                        yield Err(anyhow!("SSE error: {}", e));
                        break;
                    }
                }
            }
        };
        
        Ok(Box::pin(stream))
    }
    
    /// Create a non-streaming chat completion
    pub async fn create_chat_completion(
        &self,
        mut request: ChatCompletionRequest,
    ) -> Result<ChatCompletionResponse> {
        request.stream = false;
        
        let url = format!("{}/chat/completions", self.base_url);
        
        let response = self.client
            .post(&url)
            .json(&request)
            .send()
            .await?;
        
        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await?;
            
            // Try to parse as error response
            if let Ok(error_resp) = serde_json::from_str::<ErrorResponse>(&text) {
                return Err(anyhow!("API error {}: {}", status, error_resp.error.message));
            }
            
            return Err(anyhow!("API error {}: {}", status, text));
        }
        
        let response = response.json::<ChatCompletionResponse>().await?;
        Ok(response)
    }
}