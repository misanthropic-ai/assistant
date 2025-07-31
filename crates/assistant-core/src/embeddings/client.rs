use anyhow::Result;
use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};

use super::EmbeddingClient;

/// OpenAI embedding models
#[derive(Debug, Clone)]
pub enum OpenAIEmbeddingModel {
    Ada002,
    TextEmbedding3Small,
    TextEmbedding3Large,
    Custom(String),
}

impl OpenAIEmbeddingModel {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Ada002 => "text-embedding-ada-002",
            Self::TextEmbedding3Small => "text-embedding-3-small",
            Self::TextEmbedding3Large => "text-embedding-3-large",
            Self::Custom(s) => s,
        }
    }
    
    pub fn dimension(&self) -> usize {
        match self {
            Self::Ada002 => 1536,
            Self::TextEmbedding3Small => 1536,
            Self::TextEmbedding3Large => 3072,
            Self::Custom(_) => 1536, // Default assumption
        }
    }
}

/// OpenAI embeddings client
#[derive(Clone)]
pub struct OpenAIEmbeddingClient {
    client: Client,
    api_key: String,
    base_url: String,
    model: OpenAIEmbeddingModel,
}

#[derive(Serialize)]
struct EmbeddingRequest {
    input: Vec<String>,
    model: String,
}

#[derive(Deserialize)]
struct EmbeddingResponse {
    data: Vec<EmbeddingData>,
}

#[derive(Deserialize)]
struct EmbeddingData {
    embedding: Vec<f32>,
}

impl OpenAIEmbeddingClient {
    pub fn new(api_key: String, base_url: String, model: OpenAIEmbeddingModel) -> Self {
        Self {
            client: Client::new(),
            api_key,
            base_url,
            model,
        }
    }
    
    pub fn new_default(api_key: String) -> Self {
        Self::new(
            api_key,
            "https://api.openai.com/v1".to_string(),
            OpenAIEmbeddingModel::Ada002,
        )
    }
}

#[async_trait]
impl EmbeddingClient for OpenAIEmbeddingClient {
    async fn embed_batch(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(vec![]);
        }
        
        let request = EmbeddingRequest {
            input: texts.to_vec(),
            model: self.model.as_str().to_string(),
        };
        
        let response = self
            .client
            .post(format!("{}/embeddings", self.base_url))
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&request)
            .send()
            .await?;
        
        if !response.status().is_success() {
            let error_text = response.text().await?;
            return Err(anyhow::anyhow!("OpenAI API error: {}", error_text));
        }
        
        let embedding_response: EmbeddingResponse = response.json().await?;
        
        Ok(embedding_response
            .data
            .into_iter()
            .map(|d| d.embedding)
            .collect())
    }
    
    fn dimension(&self) -> usize {
        self.model.dimension()
    }
}