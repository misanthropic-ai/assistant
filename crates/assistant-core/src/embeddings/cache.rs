use anyhow::Result;
use lru::LruCache;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::num::NonZeroUsize;
use std::sync::Arc;
use tokio::sync::Mutex;

/// Simple in-memory cache for embeddings
pub struct EmbeddingCache {
    cache: Arc<Mutex<LruCache<u64, Vec<f32>>>>,
}

impl EmbeddingCache {
    /// Create a new embedding cache with the specified capacity
    pub fn new(capacity: usize) -> Result<Self> {
        let capacity = NonZeroUsize::new(capacity)
            .ok_or_else(|| anyhow::anyhow!("Cache capacity must be greater than 0"))?;
        
        Ok(Self {
            cache: Arc::new(Mutex::new(LruCache::new(capacity))),
        })
    }
    
    /// Create a cache with default capacity (1000 entries)
    pub fn default() -> Self {
        Self::new(1000).expect("Default capacity is valid")
    }
    
    /// Get an embedding from the cache
    pub async fn get(&self, text: &str) -> Option<Vec<f32>> {
        let key = self.hash_text(text);
        let mut cache = self.cache.lock().await;
        cache.get(&key).cloned()
    }
    
    /// Put an embedding in the cache
    pub async fn put(&self, text: &str, embedding: Vec<f32>) {
        let key = self.hash_text(text);
        let mut cache = self.cache.lock().await;
        cache.put(key, embedding);
    }
    
    /// Get multiple embeddings from the cache
    /// Returns a vector of Option<Vec<f32>> in the same order as the input texts
    pub async fn get_batch(&self, texts: &[String]) -> Vec<Option<Vec<f32>>> {
        let mut cache = self.cache.lock().await;
        texts
            .iter()
            .map(|text| {
                let key = self.hash_text(text);
                cache.get(&key).cloned()
            })
            .collect()
    }
    
    /// Put multiple embeddings in the cache
    pub async fn put_batch(&self, texts: &[String], embeddings: Vec<Vec<f32>>) {
        if texts.len() != embeddings.len() {
            tracing::warn!(
                "Text and embedding batch sizes don't match: {} vs {}",
                texts.len(),
                embeddings.len()
            );
            return;
        }
        
        let mut cache = self.cache.lock().await;
        for (text, embedding) in texts.iter().zip(embeddings.into_iter()) {
            let key = self.hash_text(text);
            cache.put(key, embedding);
        }
    }
    
    /// Clear the cache
    pub async fn clear(&self) {
        let mut cache = self.cache.lock().await;
        cache.clear();
    }
    
    /// Get cache statistics
    pub async fn stats(&self) -> CacheStats {
        let cache = self.cache.lock().await;
        CacheStats {
            capacity: cache.cap().get(),
            size: cache.len(),
        }
    }
    
    /// Hash a text string to a cache key
    fn hash_text(&self, text: &str) -> u64 {
        let mut hasher = DefaultHasher::new();
        text.hash(&mut hasher);
        hasher.finish()
    }
}

#[derive(Debug, Clone)]
pub struct CacheStats {
    pub capacity: usize,
    pub size: usize,
}

/// Wrapper for an embedding client with caching
pub struct CachedEmbeddingClient<T: super::EmbeddingClient> {
    client: T,
    cache: EmbeddingCache,
}

impl<T: super::EmbeddingClient> CachedEmbeddingClient<T> {
    pub fn new(client: T, cache_capacity: usize) -> Result<Self> {
        Ok(Self {
            client,
            cache: EmbeddingCache::new(cache_capacity)?,
        })
    }
    
    pub async fn stats(&self) -> CacheStats {
        self.cache.stats().await
    }
    
    pub async fn clear_cache(&self) {
        self.cache.clear().await;
    }
}

#[async_trait::async_trait]
impl<T: super::EmbeddingClient> super::EmbeddingClient for CachedEmbeddingClient<T> {
    async fn embed_batch(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(vec![]);
        }
        
        // Check cache for all texts
        let cached_results = self.cache.get_batch(texts).await;
        
        // Find texts that need embedding
        let mut texts_to_embed = Vec::new();
        let mut indices_to_embed = Vec::new();
        
        for (i, (text, cached)) in texts.iter().zip(cached_results.iter()).enumerate() {
            if cached.is_none() {
                texts_to_embed.push(text.clone());
                indices_to_embed.push(i);
            }
        }
        
        // If all are cached, return cached results
        if texts_to_embed.is_empty() {
            return Ok(cached_results.into_iter().map(|opt| opt.unwrap()).collect());
        }
        
        // Embed missing texts
        let new_embeddings = self.client.embed_batch(&texts_to_embed).await?;
        
        // Cache new embeddings
        self.cache.put_batch(&texts_to_embed, new_embeddings.clone()).await;
        
        // Combine cached and new results
        let mut results = vec![vec![]; texts.len()];
        let mut new_embedding_iter = new_embeddings.into_iter();
        
        for (i, cached) in cached_results.into_iter().enumerate() {
            if let Some(embedding) = cached {
                results[i] = embedding;
            } else {
                results[i] = new_embedding_iter.next().unwrap();
            }
        }
        
        Ok(results)
    }
    
    fn dimension(&self) -> usize {
        self.client.dimension()
    }
}