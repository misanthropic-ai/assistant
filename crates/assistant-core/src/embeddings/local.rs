use anyhow::Result;
use async_trait::async_trait;
use candle_core::{Device, Tensor};
use candle_transformers::models::bert::BertModel;
use std::path::PathBuf;
use tokenizers::Tokenizer;

use super::{device::detect_best_device, EmbeddingClient};

/// Local embedding models
#[derive(Debug, Clone)]
pub enum LocalEmbeddingModel {
    /// all-MiniLM-L6-v2: Fast, 384 dimensions
    AllMiniLmL6V2,
    /// GTE-small: Good quality, 512 dimensions
    GteSmall,
    /// BGE-small-en: Excellent quality, 384 dimensions
    BgeSmallEn,
    /// Custom model path
    Custom { path: PathBuf, dimension: usize },
}

impl LocalEmbeddingModel {
    pub fn model_id(&self) -> &str {
        match self {
            Self::AllMiniLmL6V2 => "sentence-transformers/all-MiniLM-L6-v2",
            Self::GteSmall => "thenlper/gte-small",
            Self::BgeSmallEn => "BAAI/bge-small-en-v1.5",
            Self::Custom { .. } => "custom",
        }
    }
    
    pub fn dimension(&self) -> usize {
        match self {
            Self::AllMiniLmL6V2 => 384,
            Self::GteSmall => 512,
            Self::BgeSmallEn => 384,
            Self::Custom { dimension, .. } => *dimension,
        }
    }
    
    pub fn cache_dir() -> Result<PathBuf> {
        let home_dir = dirs::home_dir()
            .ok_or_else(|| anyhow::anyhow!("Could not determine home directory"))?;
        Ok(home_dir.join(".assistant").join("models"))
    }
}

/// Local embeddings client using Candle
pub struct LocalEmbeddingClient {
    model: BertModel,
    tokenizer: Tokenizer,
    device: Device,
    model_type: LocalEmbeddingModel,
}

impl LocalEmbeddingClient {
    /// Create a new local embedding client
    pub async fn new(_model_type: LocalEmbeddingModel, _device: Device) -> Result<Self> {
        let cache_dir = LocalEmbeddingModel::cache_dir()?;
        std::fs::create_dir_all(&cache_dir)?;
        
        // For now, we'll use a placeholder implementation
        // In a real implementation, we would:
        // 1. Download the model weights if not cached
        // 2. Load the tokenizer
        // 3. Load the model weights
        // 4. Initialize the BERT model
        
        todo!("Implement model loading from HuggingFace Hub")
    }
    
    /// Mean pooling over token embeddings
    fn mean_pooling(&self, embeddings: &Tensor, attention_mask: &Tensor) -> Result<Tensor> {
        // Expand attention mask for broadcasting
        let mask_expanded = attention_mask.unsqueeze(2)?;
        
        // Apply mask to embeddings
        let masked_embeddings = embeddings.broadcast_mul(&mask_expanded)?;
        
        // Sum over sequence length
        let sum_embeddings = masked_embeddings.sum(1)?;
        let sum_mask = mask_expanded.sum(1)?;
        
        // Avoid division by zero
        let sum_mask = sum_mask.clamp(1e-9, f32::INFINITY)?;
        
        // Calculate mean
        Ok(sum_embeddings.broadcast_div(&sum_mask)?)
    }
}

#[async_trait]
impl EmbeddingClient for LocalEmbeddingClient {
    async fn embed_batch(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(vec![]);
        }
        
        // Tokenize texts
        let encodings = texts
            .iter()
            .map(|text| self.tokenizer.encode(text.as_str(), true))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| anyhow::anyhow!("Tokenization error: {:?}", e))?;
        
        // Convert to tensors
        let input_ids: Vec<Vec<u32>> = encodings
            .iter()
            .map(|e| e.get_ids().to_vec())
            .collect();
        
        let attention_masks: Vec<Vec<u32>> = encodings
            .iter()
            .map(|e| e.get_attention_mask().to_vec())
            .collect();
        
        // TODO: Implement batched inference
        // For now, process one at a time
        let mut embeddings = Vec::new();
        
        for (ids, mask) in input_ids.iter().zip(attention_masks.iter()) {
            // Convert to tensors
            let input_ids = Tensor::new(ids.as_slice(), &self.device)?
                .unsqueeze(0)?;
            let attention_mask = Tensor::new(mask.as_slice(), &self.device)?
                .unsqueeze(0)?;
            
            // Forward pass - BERT model expects optional token_type_ids
            let output = self.model.forward(&input_ids, &attention_mask, None)?;
            
            // Mean pooling
            let pooled = self.mean_pooling(&output, &attention_mask)?;
            
            // Convert to Vec<f32>
            let embedding: Vec<f32> = pooled.squeeze(0)?.to_vec1()?;
            embeddings.push(embedding);
        }
        
        Ok(embeddings)
    }
    
    fn dimension(&self) -> usize {
        self.model_type.dimension()
    }
}

/// Factory function to create an embedding client
pub async fn create_embedding_client(
    provider: &super::EmbeddingProvider,
    api_key: Option<String>,
    base_url: Option<String>,
) -> Result<Box<dyn EmbeddingClient>> {
    match provider {
        super::EmbeddingProvider::OpenAI { model } => {
            let api_key = api_key.ok_or_else(|| anyhow::anyhow!("OpenAI API key required"))?;
            let base_url = base_url.unwrap_or_else(|| "https://api.openai.com/v1".to_string());
            let model = super::client::OpenAIEmbeddingModel::Custom(model.clone());
            
            Ok(Box::new(super::client::OpenAIEmbeddingClient::new(
                api_key,
                base_url,
                model,
            )))
        }
        super::EmbeddingProvider::Local { model } => {
            let model_type = match model.as_str() {
                "all-MiniLM-L6-v2" => LocalEmbeddingModel::AllMiniLmL6V2,
                "gte-small" => LocalEmbeddingModel::GteSmall,
                "bge-small-en" => LocalEmbeddingModel::BgeSmallEn,
                _ => {
                    return Err(anyhow::anyhow!(
                        "Unknown local model: {}. Supported: all-MiniLM-L6-v2, gte-small, bge-small-en",
                        model
                    ));
                }
            };
            
            let device = detect_best_device(&super::device::DevicePreference::Auto)?;
            let client = LocalEmbeddingClient::new(model_type, device).await?;
            
            Ok(Box::new(client))
        }
        super::EmbeddingProvider::Ollama { model } => {
            let base_url = base_url.unwrap_or_else(|| "http://localhost:11434".to_string());
            let model = match model.as_str() {
                "mxbai-embed-large" => super::ollama::OllamaEmbeddingModel::MxbaiEmbedLarge,
                other => super::ollama::OllamaEmbeddingModel::Custom(other.to_string()),
            };
            
            Ok(Box::new(super::ollama::OllamaEmbeddingClient::new(
                base_url,
                model,
            )))
        }
    }
}