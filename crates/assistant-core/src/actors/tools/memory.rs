use ractor::{Actor, ActorRef, ActorProcessingErr};
use serde::{Deserialize, Serialize};
use crate::config::Config;
use crate::messages::{ToolMessage, ChatMessage};
use crate::persistence::Database;
use crate::embeddings::{
    EmbeddingClient, 
    cache::CachedEmbeddingClient,
    client::OpenAIEmbeddingClient,
    find_top_k_similar,
};
use anyhow::Result;
use chrono::Utc;
use sqlx::Row;
use std::sync::Arc;
use uuid::Uuid;

/// Actor for memory/context management with semantic search
pub struct MemoryActor {
    config: Config,
    db: Arc<Database>,
    embedding_client: Option<Arc<dyn EmbeddingClient + Send + Sync>>,
}

/// Memory state
pub struct MemoryState {
    embedding_client: Option<Arc<dyn EmbeddingClient + Send + Sync>>,
    db: Arc<Database>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SearchMode {
    /// Hybrid search combining keyword and semantic (default)
    Hybrid,
    /// Semantic-only search using embeddings
    Semantic,
    /// Keyword-only search using FTS5
    Keyword,
    /// Exact key match only
    Exact,
}

impl Default for SearchMode {
    fn default() -> Self {
        SearchMode::Hybrid
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum MemoryOperation {
    /// Store a memory with automatic key generation
    Store { 
        content: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        metadata: Option<serde_json::Value>,
    },
    /// Store with explicit key
    StoreWithKey { 
        key: String, 
        content: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        metadata: Option<serde_json::Value>,
    },
    /// Retrieve by key
    Retrieve { key: String },
    /// Update existing memory
    Update {
        key: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        content: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        metadata: Option<serde_json::Value>,
        #[serde(default)]
        merge_metadata: bool,
    },
    /// Search memories using specified mode
    Search { 
        query: String, 
        #[serde(default = "default_limit")]
        limit: usize,
        #[serde(default)]
        mode: SearchMode,
        #[serde(skip_serializing_if = "Option::is_none")]
        metadata_filter: Option<serde_json::Value>,
    },
    /// List all memory keys
    List {
        #[serde(skip_serializing_if = "Option::is_none")]
        prefix: Option<String>,
    },
    /// Delete a specific memory
    Delete { key: String },
    /// Clear all memories (optionally for current session only)
    Clear { 
        #[serde(default)]
        session_only: bool 
    },
    /// Get memory statistics
    Stats,
}

fn default_limit() -> usize {
    10
}

impl Actor for MemoryActor {
    type Msg = ToolMessage;
    type State = MemoryState;
    type Arguments = Config;
    
    async fn pre_start(
        &self,
        _myself: ActorRef<Self::Msg>,
        _config: Self::Arguments,
    ) -> Result<Self::State, ActorProcessingErr> {
        tracing::debug!("Memory actor starting");
        
        Ok(MemoryState {
            embedding_client: self.embedding_client.clone(),
            db: self.db.clone(),
        })
    }
    
    async fn handle(
        &self,
        _myself: ActorRef<Self::Msg>,
        msg: Self::Msg,
        state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        match msg {
            ToolMessage::Execute { id, params, chat_ref } => {
                tracing::info!("Memory tool execution with params: {:?}", params);
                
                // Parse operation
                let operation: MemoryOperation = match serde_json::from_value(params) {
                    Ok(op) => op,
                    Err(e) => {
                        chat_ref.send_message(ChatMessage::ToolResult {
                            id,
                            result: format!("Error: Invalid parameters - {}", e),
                        })?;
                        return Ok(());
                    }
                };
                
                // Handle different operations differently
                match operation {
                    // Fire-and-forget operations - respond immediately
                    MemoryOperation::Store { content, metadata } => {
                        let key = Uuid::new_v4().to_string();
                        
                        // Send response immediately
                        chat_ref.send_message(ChatMessage::ToolResult {
                            id,
                            result: format!("Stored memory with key: {}", key),
                        })?;
                        
                        // Then do the actual work
                        if let Err(e) = self.store_memory(&key, &content, metadata, state).await {
                            tracing::error!("Failed to store memory {}: {}", key, e);
                        }
                    }
                    
                    MemoryOperation::StoreWithKey { key, content, metadata } => {
                        // Send response immediately
                        chat_ref.send_message(ChatMessage::ToolResult {
                            id,
                            result: format!("Stored memory with key: {}", key),
                        })?;
                        
                        // Then do the actual work
                        if let Err(e) = self.store_memory(&key, &content, metadata, state).await {
                            tracing::error!("Failed to store memory {}: {}", key, e);
                        }
                    }
                    
                    MemoryOperation::Update { key, content, metadata, merge_metadata } => {
                        // Send response immediately
                        chat_ref.send_message(ChatMessage::ToolResult {
                            id,
                            result: format!("Updated memory: {}", key),
                        })?;
                        
                        // Then do the actual work
                        if let Err(e) = self.update_memory(&key, content.as_deref(), metadata, merge_metadata, state).await {
                            tracing::error!("Failed to update memory {}: {}", key, e);
                        }
                    }
                    
                    MemoryOperation::Delete { key } => {
                        // Send response immediately
                        chat_ref.send_message(ChatMessage::ToolResult {
                            id,
                            result: format!("Deleted memory: {}", key),
                        })?;
                        
                        // Then do the actual work
                        if let Err(e) = self.delete_memory(&key, state).await {
                            tracing::error!("Failed to delete memory {}: {}", key, e);
                        }
                    }
                    
                    MemoryOperation::Clear { session_only } => {
                        // Send response immediately
                        chat_ref.send_message(ChatMessage::ToolResult {
                            id,
                            result: if session_only {
                                "Cleared session memories".to_string()
                            } else {
                                "Cleared all memories".to_string()
                            },
                        })?;
                        
                        // Then do the actual work
                        if let Err(e) = self.clear_memories(session_only, state).await {
                            tracing::error!("Failed to clear memories: {}", e);
                        }
                    }
                    
                    // Operations that need to return data - handle synchronously
                    MemoryOperation::Retrieve { .. } |
                    MemoryOperation::Search { .. } |
                    MemoryOperation::List { .. } |
                    MemoryOperation::Stats => {
                        // Execute operation synchronously since we need the result
                        let result = match self.execute_operation(operation, state).await {
                            Ok(result) => result,
                            Err(e) => format!("Error: {}", e),
                        };
                        
                        // Send result back to chat actor
                        chat_ref.send_message(ChatMessage::ToolResult {
                            id,
                            result,
                        })?;
                    }
                }
            }
            
            ToolMessage::Cancel { id } => {
                tracing::debug!("Cancelling memory operation {}", id);
                // Most memory operations are quick, but we could add cancellation for searches
            }
            
            ToolMessage::StreamUpdate { .. } => {
                // Memory doesn't stream updates currently
            }
        }
        
        Ok(())
    }
}

impl MemoryActor {
    pub async fn new(config: Config) -> Result<Self> {
        // Initialize database
        let db_path = config.session.database_path.as_ref()
            .map(|p| p.clone())
            .unwrap_or_else(|| Database::default_path().unwrap());
        
        let db = Arc::new(Database::new(&db_path).await?);
        
        // Initialize embedding client based on config
        let embedding_client = if let Some(model_config) = config.embeddings.models.get(&config.embeddings.default_model) {
            match model_config.provider.as_str() {
                "openai" => {
                    // Get API key from model config or environment
                    let api_key = model_config.api_key.clone()
                        .or_else(|| std::env::var("OPENAI_API_KEY").ok())
                        .ok_or_else(|| anyhow::anyhow!("OpenAI API key not found in config or OPENAI_API_KEY environment variable"))?;
                    
                    // Get base URL from model config or use default
                    let base_url = model_config.base_url.clone()
                        .unwrap_or_else(|| "https://api.openai.com/v1".to_string());
                    
                    let client = OpenAIEmbeddingClient::new(
                        api_key,
                        base_url,
                        crate::embeddings::client::OpenAIEmbeddingModel::Custom(model_config.model.clone()),
                    );
                    
                    // Wrap with cache
                    let cached_client = CachedEmbeddingClient::new(
                        client,
                        config.embeddings.cache_size,
                    )?;
                    
                    Some(Arc::new(cached_client) as Arc<dyn EmbeddingClient + Send + Sync>)
                }
                "local" => {
                    // Local embeddings not fully implemented yet
                    tracing::warn!("Local embeddings not yet implemented, memory search will be disabled");
                    None
                }
                _ => {
                    tracing::warn!("Unknown embedding provider: {}", model_config.provider);
                    None
                }
            }
        } else {
            tracing::warn!("Default embedding model '{}' not found in config", config.embeddings.default_model);
            None
        };
        
        Ok(Self {
            config,
            db,
            embedding_client,
        })
    }
    
    async fn execute_operation(
        &self,
        operation: MemoryOperation,
        state: &MemoryState,
    ) -> Result<String> {
        match operation {
            MemoryOperation::Store { content, metadata } => {
                let key = Uuid::new_v4().to_string();
                self.store_memory(&key, &content, metadata, state).await?;
                Ok(format!("Stored memory with key: {}", key))
            }
            
            MemoryOperation::StoreWithKey { key, content, metadata } => {
                self.store_memory(&key, &content, metadata, state).await?;
                Ok(format!("Stored memory with key: {}", key))
            }
            
            MemoryOperation::Retrieve { key } => {
                match self.retrieve_memory(&key, state).await? {
                    Some((content, metadata)) => {
                        if let Some(meta) = metadata {
                            Ok(format!("{}\n\nMetadata: {}", content, serde_json::to_string_pretty(&meta)?))
                        } else {
                            Ok(content)
                        }
                    }
                    None => Ok(format!("Memory key '{}' not found", key)),
                }
            }
            
            MemoryOperation::Update { key, content, metadata, merge_metadata } => {
                match self.update_memory(&key, content.as_deref(), metadata, merge_metadata, state).await {
                    Ok(true) => Ok(format!("Updated memory: {}", key)),
                    Ok(false) => Ok(format!("Memory key '{}' not found", key)),
                    Err(e) => Err(e),
                }
            }
            
            MemoryOperation::Search { query, limit, mode, metadata_filter } => {
                let results = self.search_memories(&query, limit, mode, metadata_filter, state).await?;
                if results.is_empty() {
                    Ok("No memories found matching the query".to_string())
                } else {
                    let mut output = format!("Found {} memories:\n\n", results.len());
                    for (i, (key, content, similarity)) in results.iter().enumerate() {
                        output.push_str(&format!(
                            "{}. [{}] (similarity: {:.3})\n{}\n\n",
                            i + 1, key, similarity, 
                            // Truncate long content
                            if content.len() > 200 {
                                format!("{}...", &content[..200])
                            } else {
                                content.clone()
                            }
                        ));
                    }
                    Ok(output)
                }
            }
            
            MemoryOperation::List { prefix } => {
                let keys = self.list_memory_keys(prefix.as_deref(), state).await?;
                if keys.is_empty() {
                    Ok("No memories found".to_string())
                } else {
                    Ok(format!("Memory keys ({}):\n{}", keys.len(), keys.join("\n")))
                }
            }
            
            MemoryOperation::Delete { key } => {
                if self.delete_memory(&key, state).await? {
                    Ok(format!("Deleted memory: {}", key))
                } else {
                    Ok(format!("Memory key '{}' not found", key))
                }
            }
            
            MemoryOperation::Clear { session_only } => {
                let count = self.clear_memories(session_only, state).await?;
                Ok(format!("Cleared {} memories", count))
            }
            
            MemoryOperation::Stats => {
                let stats = self.get_memory_stats(state).await?;
                Ok(format!(
                    "Memory Statistics:\n\
                    Total memories: {}\n\
                    Total size: {} bytes\n\
                    Embeddings cached: {}",
                    stats.total_count,
                    stats.total_size,
                    stats.embeddings_cached
                ))
            }
        }
    }
    
    async fn store_memory(
        &self,
        key: &str,
        content: &str,
        metadata: Option<serde_json::Value>,
        state: &MemoryState,
    ) -> Result<()> {
        let id = Uuid::new_v4().to_string();
        let now = Utc::now();
        
        // Generate embedding if client is available
        let embedding = if let Some(client) = &state.embedding_client {
            match client.embed(content).await {
                Ok(embedding) => Some(embedding),
                Err(e) => {
                    tracing::warn!("Failed to generate embedding: {}", e);
                    None
                }
            }
        } else {
            None
        };
        
        // Serialize embedding to bytes
        let embedding_bytes = embedding.as_ref().map(|e| {
            e.iter().flat_map(|f| f.to_le_bytes()).collect::<Vec<u8>>()
        });
        
        // Store in database (memories are global)
        sqlx::query(
            r#"
            INSERT INTO memories (id, key, content, embedding, metadata, created_at, accessed_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
            ON CONFLICT(key) DO UPDATE SET
                content = excluded.content,
                embedding = excluded.embedding,
                metadata = excluded.metadata,
                accessed_at = excluded.accessed_at,
                access_count = access_count + 1
            "#,
        )
        .bind(&id)
        .bind(key)
        .bind(content)
        .bind(embedding_bytes)
        .bind(metadata.and_then(|m| serde_json::to_string(&m).ok()))
        .bind(&now)
        .bind(&now)
        .execute(state.db.pool())
        .await?;
        
        Ok(())
    }
    
    async fn update_memory(
        &self,
        key: &str,
        content: Option<&str>,
        metadata: Option<serde_json::Value>,
        merge_metadata: bool,
        state: &MemoryState,
    ) -> Result<bool> {
        // First check if the memory exists
        let existing = sqlx::query(
            "SELECT content, metadata FROM memories WHERE key = ?1"
        )
        .bind(key)
        .fetch_optional(state.db.pool())
        .await?;
        
        if existing.is_none() {
            return Ok(false);
        }
        
        let now = Utc::now();
        
        // Generate new embedding if content is being updated
        let new_embedding = if let Some(new_content) = content {
            if let Some(client) = &state.embedding_client {
                match client.embed(new_content).await {
                    Ok(embedding) => Some(embedding),
                    Err(e) => {
                        tracing::warn!("Failed to generate embedding for update: {}", e);
                        None
                    }
                }
            } else {
                None
            }
        } else {
            None
        };
        
        // Serialize embedding to bytes if we have one
        let embedding_bytes = new_embedding.as_ref().map(|e| {
            e.iter().flat_map(|f| f.to_le_bytes()).collect::<Vec<u8>>()
        });
        
        // Handle metadata merging
        let final_metadata = if merge_metadata && metadata.is_some() {
            let existing_row = existing.unwrap();
            let existing_metadata: Option<String> = existing_row.get(1);
            
            if let Some(existing_json) = existing_metadata {
                if let Ok(mut existing_obj) = serde_json::from_str::<serde_json::Value>(&existing_json) {
                    if let (Some(existing_map), Some(new_map)) = (existing_obj.as_object_mut(), metadata.as_ref().and_then(|m| m.as_object())) {
                        // Merge new fields into existing
                        for (k, v) in new_map {
                            existing_map.insert(k.clone(), v.clone());
                        }
                        Some(existing_obj)
                    } else {
                        metadata.clone()
                    }
                } else {
                    metadata.clone()
                }
            } else {
                metadata.clone()
            }
        } else {
            metadata.clone()
        };
        
        // Build dynamic update query
        let mut sql = "UPDATE memories SET accessed_at = ?1, access_count = access_count + 1".to_string();
        let mut param_count = 1;
        
        if content.is_some() {
            param_count += 1;
            sql.push_str(&format!(", content = ?{}", param_count));
        }
        
        if embedding_bytes.is_some() {
            param_count += 1;
            sql.push_str(&format!(", embedding = ?{}", param_count));
        }
        
        if merge_metadata || metadata.is_some() {
            param_count += 1;
            sql.push_str(&format!(", metadata = ?{}", param_count));
        }
        
        param_count += 1;
        sql.push_str(&format!(" WHERE key = ?{}", param_count));
        
        // Build query with dynamic parameters
        let mut query = sqlx::query(&sql).bind(&now);
        
        if let Some(c) = content {
            query = query.bind(c);
        }
        
        if let Some(e) = embedding_bytes {
            query = query.bind(e);
        }
        
        if merge_metadata || metadata.is_some() {
            query = query.bind(final_metadata.and_then(|m| serde_json::to_string(&m).ok()));
        }
        
        query = query.bind(key);
        
        let result = query.execute(state.db.pool()).await?;
        
        Ok(result.rows_affected() > 0)
    }
    
    async fn retrieve_memory(
        &self,
        key: &str,
        state: &MemoryState,
    ) -> Result<Option<(String, Option<serde_json::Value>)>> {
        let row = sqlx::query(
            r#"
            SELECT content, metadata
            FROM memories
            WHERE key = ?1
            LIMIT 1
            "#,
        )
        .bind(key)
        .fetch_optional(state.db.pool())
        .await?;
        
        if let Some(row) = row {
            // Update access time
            sqlx::query(
                r#"
                UPDATE memories
                SET accessed_at = ?1, access_count = access_count + 1
                WHERE key = ?2
                "#,
            )
            .bind(Utc::now())
            .bind(key)
            .execute(state.db.pool())
            .await?;
            
            let content: String = row.get(0);
            let metadata: Option<String> = row.get(1);
            let metadata_json = metadata.and_then(|m| serde_json::from_str(&m).ok());
            
            Ok(Some((content, metadata_json)))
        } else {
            Ok(None)
        }
    }
    
    async fn search_memories(
        &self,
        query: &str,
        limit: usize,
        mode: SearchMode,
        metadata_filter: Option<serde_json::Value>,
        state: &MemoryState,
    ) -> Result<Vec<(String, String, f32)>> {
        match mode {
            SearchMode::Exact => self.exact_search(query, limit, metadata_filter, state).await,
            SearchMode::Keyword => self.keyword_search(query, limit, metadata_filter, state).await,
            SearchMode::Semantic => self.semantic_search(query, limit, metadata_filter, state).await,
            SearchMode::Hybrid => self.hybrid_search(query, limit, metadata_filter, state).await,
        }
    }
    
    async fn exact_search(
        &self,
        query: &str,
        limit: usize,
        metadata_filter: Option<serde_json::Value>,
        state: &MemoryState,
    ) -> Result<Vec<(String, String, f32)>> {
        let mut sql = "SELECT key, content FROM memories WHERE key = ?1".to_string();
        let has_metadata_filter = metadata_filter.is_some();
        if has_metadata_filter {
            sql.push_str(" AND json_extract(metadata, '$') LIKE ?2");
        }
        sql.push_str(&format!(" LIMIT ?{}", if has_metadata_filter { 3 } else { 2 }));
        
        let mut query_builder = sqlx::query(&sql).bind(query);
        if let Some(filter) = metadata_filter {
            query_builder = query_builder.bind(format!("%{}%", filter.to_string()));
        }
        query_builder = query_builder.bind(limit as i64);
        
        let rows = query_builder.fetch_all(state.db.pool()).await?;
        
        Ok(rows.into_iter()
            .map(|row| {
                let key: String = row.get(0);
                let content: String = row.get(1);
                (key, content, 1.0) // Exact match gets score 1.0
            })
            .collect())
    }
    
    async fn keyword_search(
        &self,
        query: &str,
        limit: usize,
        metadata_filter: Option<serde_json::Value>,
        state: &MemoryState,
    ) -> Result<Vec<(String, String, f32)>> {
        // Use FTS5 for keyword search with BM25 ranking
        let mut sql = r#"
            SELECT DISTINCT m.key, m.content, bm25(memories_fts) as score
            FROM memories_fts f
            JOIN memories m ON f.key = m.key
            WHERE memories_fts MATCH ?1
        "#.to_string();
        
        let has_metadata_filter = metadata_filter.is_some();
        if has_metadata_filter {
            sql.push_str(" AND json_extract(m.metadata, '$') LIKE ?2");
        }
        sql.push_str(&format!(" ORDER BY score DESC LIMIT ?{}", if has_metadata_filter { 3 } else { 2 }));
        
        tracing::debug!("Keyword search SQL: {}", sql);
        tracing::debug!("Query: {}, Limit: {}", query, limit);
        
        let mut query_builder = sqlx::query(&sql).bind(query);
        if let Some(filter) = metadata_filter {
            query_builder = query_builder.bind(format!("%{}%", filter.to_string()));
        }
        query_builder = query_builder.bind(limit as i64);
        
        let rows = query_builder.fetch_all(state.db.pool()).await?;
        
        // Normalize BM25 scores
        let mut results: Vec<(String, String, f32)> = rows.into_iter()
            .map(|row| {
                let key: String = row.get(0);
                let content: String = row.get(1);
                let score: f64 = row.get(2);
                let score = score as f32;
                (key, content, score)
            })
            .collect();
        
        // Normalize scores to 0-1 range
        if let Some(&max_score) = results.iter().map(|(_, _, s)| s).max_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal)) {
            if max_score > 0.0 {
                for (_, _, score) in &mut results {
                    *score /= max_score;
                }
            }
        }
        Ok(results)
    }
    
    async fn semantic_search(
        &self,
        query: &str,
        limit: usize,
        metadata_filter: Option<serde_json::Value>,
        state: &MemoryState,
    ) -> Result<Vec<(String, String, f32)>> {
        if let Some(client) = &state.embedding_client {
            let query_embedding = client.embed(query).await?;
            
            // Build query with optional metadata filter
            let mut sql = r#"
                SELECT key, content, embedding
                FROM memories
                WHERE embedding IS NOT NULL
            "#.to_string();
            
            let query_builder = if let Some(filter) = &metadata_filter {
                sql.push_str(" AND json_extract(metadata, '$') LIKE ?1");
                sqlx::query(&sql).bind(format!("%{}%", filter.to_string()))
            } else {
                sqlx::query(&sql)
            };
            
            let rows = query_builder.fetch_all(state.db.pool()).await?;
            
            let mut candidates = Vec::new();
            let mut content_map = Vec::new();
            
            for row in rows {
                let key: String = row.get(0);
                let content: String = row.get(1);
                let embedding_bytes: Vec<u8> = row.get(2);
                
                // Deserialize embedding
                if embedding_bytes.len() % 4 == 0 {
                    let embedding: Vec<f32> = embedding_bytes
                        .chunks(4)
                        .map(|chunk| f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
                        .collect();
                    
                    candidates.push((key.clone(), embedding));
                    content_map.push((key, content));
                }
            }
            
            // Find top-k similar
            let results = find_top_k_similar(&query_embedding, &candidates, limit);
            
            Ok(results.into_iter()
                .map(|(key, similarity)| {
                    let content = content_map.iter()
                        .find(|(k, _)| k == &key)
                        .map(|(_, c)| c.clone())
                        .unwrap_or_default();
                    (key, content, similarity)
                })
                .collect())
        } else {
            // Fallback to keyword search if no embedding client
            self.keyword_search(query, limit, metadata_filter, state).await
        }
    }
    
    async fn hybrid_search(
        &self,
        query: &str,
        limit: usize,
        metadata_filter: Option<serde_json::Value>,
        state: &MemoryState,
    ) -> Result<Vec<(String, String, f32)>> {
        // Reciprocal Rank Fusion parameters
        const RRF_K: f32 = 60.0;
        
        // Get results from both search methods
        let keyword_results = self.keyword_search(query, limit * 2, metadata_filter.clone(), state).await?;
        let semantic_results = self.semantic_search(query, limit * 2, metadata_filter, state).await?;
        
        // Build a map to accumulate RRF scores
        let mut rrf_scores: std::collections::HashMap<String, f32> = std::collections::HashMap::new();
        let mut content_map: std::collections::HashMap<String, String> = std::collections::HashMap::new();
        
        // Add keyword search results with RRF scoring
        for (rank, (key, content, _score)) in keyword_results.iter().enumerate() {
            let rrf_score = 1.0 / (RRF_K + rank as f32 + 1.0);
            *rrf_scores.entry(key.clone()).or_insert(0.0) += rrf_score;
            content_map.insert(key.clone(), content.clone());
        }
        
        // Add semantic search results with RRF scoring
        for (rank, (key, content, _score)) in semantic_results.iter().enumerate() {
            let rrf_score = 1.0 / (RRF_K + rank as f32 + 1.0);
            *rrf_scores.entry(key.clone()).or_insert(0.0) += rrf_score;
            content_map.insert(key.clone(), content.clone());
        }
        
        // Sort by combined RRF score and take top limit
        let mut results: Vec<(String, f32)> = rrf_scores.into_iter().collect();
        results.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        results.truncate(limit);
        
        // Return results with content
        Ok(results.into_iter()
            .map(|(key, score)| {
                let content = content_map.get(&key).cloned().unwrap_or_default();
                (key, content, score)
            })
            .collect())
    }
    
    async fn list_memory_keys(
        &self,
        prefix: Option<&str>,
        state: &MemoryState,
    ) -> Result<Vec<String>> {
        let query = if let Some(prefix) = prefix {
            sqlx::query(
                r#"
                SELECT key
                FROM memories
                WHERE key LIKE ?1
                ORDER BY accessed_at DESC
                "#,
            )
            .bind(format!("{}%", prefix))
        } else {
            sqlx::query(
                r#"
                SELECT key
                FROM memories
                ORDER BY accessed_at DESC
                "#,
            )
        };
        
        let rows = query.fetch_all(state.db.pool()).await?;
        Ok(rows.into_iter().map(|row| row.get(0)).collect())
    }
    
    async fn delete_memory(&self, key: &str, state: &MemoryState) -> Result<bool> {
        let result = sqlx::query(
            r#"
            DELETE FROM memories
            WHERE key = ?1
            "#,
        )
        .bind(key)
        .execute(state.db.pool())
        .await?;
        
        Ok(result.rows_affected() > 0)
    }
    
    async fn clear_memories(&self, _session_only: bool, state: &MemoryState) -> Result<u64> {
        // Memories are global, so we always clear all memories
        let result = sqlx::query("DELETE FROM memories")
            .execute(state.db.pool())
            .await?;
        
        Ok(result.rows_affected())
    }
    
    async fn get_memory_stats(&self, state: &MemoryState) -> Result<MemoryStats> {
        let total_row = sqlx::query(
            r#"
            SELECT COUNT(*), SUM(LENGTH(content))
            FROM memories
            "#,
        )
        .fetch_one(state.db.pool())
        .await?;
        
        let embeddings_row = sqlx::query(
            r#"
            SELECT COUNT(*)
            FROM memories
            WHERE embedding IS NOT NULL
            "#,
        )
        .fetch_one(state.db.pool())
        .await?;
        
        Ok(MemoryStats {
            total_count: total_row.get(0),
            total_size: total_row.get::<Option<i64>, _>(1).unwrap_or(0) as u64,
            embeddings_cached: embeddings_row.get(0),
        })
    }
}

#[derive(Debug)]
struct MemoryStats {
    total_count: i64,
    total_size: u64,
    embeddings_cached: i64,
}