pub mod cache;
pub mod client;
pub mod device;
pub mod local;
pub mod ollama;

use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "type")]
pub enum EmbeddingProvider {
    OpenAI { model: String },
    Local { model: String },
    Ollama { model: String },
}

impl Default for EmbeddingProvider {
    fn default() -> Self {
        EmbeddingProvider::OpenAI {
            model: "text-embedding-3-small".to_string(),
        }
    }
}

#[async_trait]
pub trait EmbeddingClient: Send + Sync {
    /// Generate embeddings for a batch of texts
    async fn embed_batch(&self, texts: &[String]) -> Result<Vec<Vec<f32>>>;
    
    /// Generate embedding for a single text
    async fn embed(&self, text: &str) -> Result<Vec<f32>> {
        let batch = self.embed_batch(&[text.to_string()]).await?;
        batch.into_iter()
            .next()
            .ok_or_else(|| anyhow::anyhow!("No embedding returned"))
    }
    
    /// Get the dimension of embeddings produced by this client
    fn dimension(&self) -> usize;
}

/// Calculate cosine similarity between two embeddings
pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    assert_eq!(a.len(), b.len(), "Embeddings must have the same dimension");
    
    let dot_product: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    
    if norm_a == 0.0 || norm_b == 0.0 {
        0.0
    } else {
        dot_product / (norm_a * norm_b)
    }
}

/// Find top-k most similar embeddings
pub fn find_top_k_similar(
    query: &[f32],
    embeddings: &[(String, Vec<f32>)],
    k: usize,
) -> Vec<(String, f32)> {
    let mut similarities: Vec<(String, f32)> = embeddings
        .iter()
        .map(|(id, embedding)| {
            let similarity = cosine_similarity(query, embedding);
            (id.clone(), similarity)
        })
        .collect();
    
    similarities.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    similarities.truncate(k);
    
    similarities
}