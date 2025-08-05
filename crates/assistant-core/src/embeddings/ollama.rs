use anyhow::Result;
use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};

use super::EmbeddingClient;

/// Ollama embedding models
#[derive(Debug, Clone)]
pub enum OllamaEmbeddingModel {
    MxbaiEmbedLarge,
    Custom(String),
}

impl OllamaEmbeddingModel {
    pub fn as_str(&self) -> &str {
        match self {
            Self::MxbaiEmbedLarge => "mxbai-embed-large",
            Self::Custom(s) => s,
        }
    }
    
    pub fn dimension(&self) -> usize {
        match self {
            Self::MxbaiEmbedLarge => 1024,
            Self::Custom(_) => 1024, // Default assumption
        }
    }
}

/// Ollama embeddings client
#[derive(Clone)]
pub struct OllamaEmbeddingClient {
    client: Client,
    base_url: String,
    model: OllamaEmbeddingModel,
}

#[derive(Serialize)]
struct EmbeddingRequest {
    model: String,
    input: String,
}

#[derive(Deserialize)]
struct EmbeddingResponse {
    embeddings: Vec<Vec<f32>>,
}

impl OllamaEmbeddingClient {
    pub fn new(base_url: String, model: OllamaEmbeddingModel) -> Self {
        Self {
            client: Client::new(),
            base_url,
            model,
        }
    }
    
    pub fn new_default() -> Self {
        Self::new(
            "http://localhost:11434".to_string(),
            OllamaEmbeddingModel::MxbaiEmbedLarge,
        )
    }
}

#[async_trait]
impl EmbeddingClient for OllamaEmbeddingClient {
    async fn embed_batch(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(vec![]);
        }
        
        // Ollama doesn't support batch embeddings, so we need to make multiple requests
        let mut embeddings = Vec::with_capacity(texts.len());
        
        for text in texts {
            let request = EmbeddingRequest {
                model: self.model.as_str().to_string(),
                input: text.clone(),
            };
            
            let response = self
                .client
                .post(format!("{}/api/embed", self.base_url))
                .json(&request)
                .send()
                .await?;
            
            if !response.status().is_success() {
                let error_text = response.text().await?;
                return Err(anyhow::anyhow!("Ollama API error: {}", error_text));
            }
            
            let embedding_response: EmbeddingResponse = response.json().await?;
            
            // Ollama returns embeddings as [[embedding]], we need the first one
            if let Some(embedding) = embedding_response.embeddings.into_iter().next() {
                embeddings.push(embedding);
            } else {
                return Err(anyhow::anyhow!("No embedding returned from Ollama"));
            }
        }
        
        Ok(embeddings)
    }
    
    fn dimension(&self) -> usize {
        self.model.dimension()
    }
}