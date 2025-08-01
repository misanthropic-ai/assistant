use ractor::{Actor, ActorRef, ActorProcessingErr};
use serde::{Deserialize, Serialize};
use serde_json::json;
use crate::config::Config;
use crate::messages::{ToolMessage, ChatMessage};
use crate::persistence::Database;
use crate::embeddings::{
    EmbeddingClient, 
    cache::CachedEmbeddingClient,
    client::{OpenAIEmbeddingClient, OpenAIEmbeddingModel},
    find_top_k_similar,
};
use anyhow::Result;
use chrono::{Utc, DateTime};
use sqlx::Row;
use std::sync::Arc;
use std::collections::HashMap;

/// Actor for knowledge synthesis and intelligent information retrieval
pub struct KnowledgeAgentActor {
    config: Config,
    db: Arc<Database>,
    embedding_client: Option<Arc<dyn EmbeddingClient + Send + Sync>>,
}

/// Knowledge agent state
pub struct KnowledgeAgentState {
    embedding_client: Option<Arc<dyn EmbeddingClient + Send + Sync>>,
    db: Arc<Database>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum KnowledgeAction {
    /// Search for knowledge across all sources
    Search {
        query: String,
        #[serde(default = "default_limit")]
        limit: usize,
        #[serde(default)]
        source_filter: Option<Vec<KnowledgeSource>>,
        #[serde(default)]
        time_filter: Option<TimeFilter>,
    },
    /// Get detailed information about a specific item
    GetDetails {
        source: KnowledgeSource,
        id: String,
    },
    /// Analyze patterns and connections in the knowledge base
    Analyze {
        topic: String,
        #[serde(default = "default_analysis_depth")]
        depth: AnalysisDepth,
    },
    /// Synthesize knowledge on a topic
    Synthesize {
        topic: String,
        #[serde(default)]
        include_examples: bool,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KnowledgeSource {
    Memory,
    ChatHistory,
    Todo,
    Session,
    All,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimeFilter {
    #[serde(default)]
    start: Option<DateTime<Utc>>,
    #[serde(default)]
    end: Option<DateTime<Utc>>,
    #[serde(default)]
    relative: Option<RelativeTime>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RelativeTime {
    LastHour,
    LastDay,
    LastWeek,
    LastMonth,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AnalysisDepth {
    Quick,
    Standard,
    Deep,
}

fn default_limit() -> usize { 20 }
fn default_analysis_depth() -> AnalysisDepth { AnalysisDepth::Standard }

impl KnowledgeAgentActor {
    pub async fn new(config: Config) -> Result<Self> {
        let db_path = config.session.database_path.as_ref()
            .map(|p| p.clone())
            .unwrap_or_else(|| Database::default_path().unwrap());
        
        let db = Arc::new(Database::new(&db_path).await?);
        
        // Initialize embedding client if configured
        let embedding_client = if let Some(model_config) = config.embeddings.models.get(&config.embeddings.default_model) {
            if let (Some(api_key), Some(base_url)) = (&model_config.api_key, &model_config.base_url) {
                let client = OpenAIEmbeddingClient::new(
                    api_key.clone(),
                    base_url.clone(),
                    OpenAIEmbeddingModel::Custom(model_config.model.clone()),
                );
                match CachedEmbeddingClient::new(client, config.embeddings.cache_size) {
                    Ok(cached_client) => Some(Arc::new(cached_client) as Arc<dyn EmbeddingClient + Send + Sync>),
                    Err(e) => {
                        tracing::error!("Failed to create cached embedding client: {}", e);
                        None
                    }
                }
            } else {
                None
            }
        } else {
            None
        };
        
        Ok(Self {
            config,
            db,
            embedding_client,
        })
    }
    
    async fn search_knowledge(
        &self,
        query: &str,
        limit: usize,
        source_filter: Option<Vec<KnowledgeSource>>,
        _time_filter: Option<TimeFilter>,
        state: &KnowledgeAgentState,
    ) -> Result<serde_json::Value> {
        let sources = source_filter.unwrap_or_else(|| vec![KnowledgeSource::All]);
        let mut all_results = Vec::new();
        
        for source in sources {
            match source {
                KnowledgeSource::All => {
                    // Search all sources
                    let memories = self.search_memories(query, limit, state).await?;
                    all_results.extend(memories);
                    
                    let messages = self.search_chat_history(query, limit, state).await?;
                    all_results.extend(messages);
                    
                    let todos = self.search_todos(query, limit, state).await?;
                    all_results.extend(todos);
                    
                    let sessions = self.search_sessions(query, limit, state).await?;
                    all_results.extend(sessions);
                }
                KnowledgeSource::Memory => {
                    let memories = self.search_memories(query, limit, state).await?;
                    all_results.extend(memories);
                }
                KnowledgeSource::ChatHistory => {
                    let messages = self.search_chat_history(query, limit, state).await?;
                    all_results.extend(messages);
                }
                KnowledgeSource::Todo => {
                    let todos = self.search_todos(query, limit, state).await?;
                    all_results.extend(todos);
                }
                KnowledgeSource::Session => {
                    let sessions = self.search_sessions(query, limit, state).await?;
                    all_results.extend(sessions);
                }
            }
        }
        
        // Sort by relevance and limit
        all_results.sort_by(|a, b| {
            let score_a = a.get("relevance_score").and_then(|v| v.as_f64()).unwrap_or(0.0);
            let score_b = b.get("relevance_score").and_then(|v| v.as_f64()).unwrap_or(0.0);
            score_b.partial_cmp(&score_a).unwrap()
        });
        all_results.truncate(limit);
        
        Ok(json!({
            "query": query,
            "total_results": all_results.len(),
            "results": all_results
        }))
    }
    
    async fn search_memories(&self, query: &str, limit: usize, state: &KnowledgeAgentState) -> Result<Vec<serde_json::Value>> {
        let mut results = Vec::new();
        
        // Try semantic search first if embeddings are available
        if let Some(client) = &state.embedding_client {
            match client.embed(query).await {
                Ok(query_embedding) => {
                    // Fetch memories with embeddings
                    let rows = sqlx::query(
                        "SELECT id, key, content, embedding FROM memories 
                         WHERE embedding IS NOT NULL 
                         ORDER BY accessed_at DESC LIMIT 100"
                    )
                    .fetch_all(state.db.pool())
                    .await?;
                    
                    let mut candidates = Vec::new();
                    let mut content_map = HashMap::new();
                    
                    for row in rows {
                        let id: String = row.get("id");
                        let key: String = row.get("key");
                        let content: String = row.get("content");
                        let embedding_blob: Vec<u8> = row.get("embedding");
                        
                        // Try to deserialize the embedding (stored as JSON bytes)
                        if let Ok(embedding_str) = std::str::from_utf8(&embedding_blob) {
                            if let Ok(embedding) = serde_json::from_str::<Vec<f32>>(embedding_str) {
                                candidates.push((key.clone(), embedding));
                                content_map.insert(key.clone(), (id, content));
                            }
                        }
                    }
                    
                    // Find similar memories
                    let similar = find_top_k_similar(&query_embedding, &candidates, limit.min(10));
                    
                    for (key, score) in similar {
                        if let Some((id, content)) = content_map.get(&key) {
                            results.push(json!({
                                "source": "memory",
                                "id": id,
                                "key": key,
                                "content": content,
                                "relevance_score": score,
                                "match_type": "semantic"
                            }));
                        }
                    }
                }
                Err(_) => {
                    // Fall back to keyword search
                }
            }
        }
        
        // Add keyword search results
        let keyword_results = sqlx::query(
            "SELECT m.id, m.key, m.content, 
             snippet(memories_fts, 1, '<match>', '</match>', '...', 20) as snippet
             FROM memories m
             JOIN memories_fts ON m.key = memories_fts.key
             WHERE memories_fts MATCH ?1
             ORDER BY rank LIMIT ?2"
        )
        .bind(query)
        .bind(limit as i32)
        .fetch_all(state.db.pool())
        .await?;
        
        for row in keyword_results {
            let id: String = row.get("id");
            let key: String = row.get("key");
            let content: String = row.get("content");
            let snippet: String = row.get("snippet");
            
            // Check if we already have this memory from semantic search
            if !results.iter().any(|r| r.get("id") == Some(&json!(id))) {
                results.push(json!({
                    "source": "memory",
                    "id": id,
                    "key": key,
                    "content": content,
                    "snippet": snippet,
                    "relevance_score": 0.8,
                    "match_type": "keyword"
                }));
            }
        }
        
        Ok(results)
    }
    
    async fn search_chat_history(&self, query: &str, limit: usize, state: &KnowledgeAgentState) -> Result<Vec<serde_json::Value>> {
        let results = sqlx::query(
            "SELECT cm.id, cm.session_id, cm.role, cm.content, cm.created_at,
             s.name as session_name, s.workspace_path,
             snippet(chat_messages_fts, 0, '<match>', '</match>', '...', 30) as snippet
             FROM chat_messages cm
             JOIN sessions s ON cm.session_id = s.id
             JOIN chat_messages_fts ON cm.rowid = chat_messages_fts.rowid
             WHERE chat_messages_fts MATCH ?1
             ORDER BY rank, cm.created_at DESC LIMIT ?2"
        )
        .bind(query)
        .bind(limit as i32)
        .fetch_all(state.db.pool())
        .await?;
        
        let mut json_results = Vec::new();
        for row in results {
            let id: String = row.get("id");
            let session_id: String = row.get("session_id");
            let role: String = row.get("role");
            let content: Option<String> = row.get("content");
            let created_at: DateTime<Utc> = row.get("created_at");
            let session_name: Option<String> = row.get("session_name");
            let workspace_path: Option<String> = row.get("workspace_path");
            let snippet: String = row.get("snippet");
            
            json_results.push(json!({
                "source": "chat_history",
                "id": id,
                "session_id": session_id,
                "session_name": session_name,
                "workspace_path": workspace_path,
                "role": role,
                "content": content,
                "snippet": snippet,
                "created_at": created_at.to_rfc3339(),
                "relevance_score": 0.85,
                "match_type": "keyword"
            }));
        }
        
        Ok(json_results)
    }
    
    async fn search_todos(&self, query: &str, limit: usize, state: &KnowledgeAgentState) -> Result<Vec<serde_json::Value>> {
        let results = sqlx::query(
            "SELECT t.id, t.session_id, t.content, t.status, t.priority, t.created_at,
             s.name as session_name, s.workspace_path
             FROM todos t
             JOIN sessions s ON t.session_id = s.id
             WHERE t.content LIKE ?1
             ORDER BY t.created_at DESC LIMIT ?2"
        )
        .bind(format!("%{}%", query))
        .bind(limit as i32)
        .fetch_all(state.db.pool())
        .await?;
        
        let mut json_results = Vec::new();
        for row in results {
            let id: String = row.get("id");
            let session_id: String = row.get("session_id");
            let content: String = row.get("content");
            let status: String = row.get("status");
            let priority: String = row.get("priority");
            let created_at: DateTime<Utc> = row.get("created_at");
            let session_name: Option<String> = row.get("session_name");
            let workspace_path: Option<String> = row.get("workspace_path");
            
            json_results.push(json!({
                "source": "todo",
                "id": id,
                "session_id": session_id,
                "session_name": session_name,
                "workspace_path": workspace_path,
                "content": content,
                "status": status,
                "priority": priority,
                "created_at": created_at.to_rfc3339(),
                "relevance_score": 0.75,
                "match_type": "keyword"
            }));
        }
        
        Ok(json_results)
    }
    
    async fn search_sessions(&self, query: &str, limit: usize, state: &KnowledgeAgentState) -> Result<Vec<serde_json::Value>> {
        let results = sqlx::query(
            "SELECT s.id, s.name, s.summary, s.workspace_path, s.created_at, s.last_accessed,
             snippet(sessions_fts, 0, '<match>', '</match>', '...', 30) as name_snippet,
             snippet(sessions_fts, 1, '<match>', '</match>', '...', 30) as summary_snippet
             FROM sessions s
             JOIN sessions_fts ON s.rowid = sessions_fts.rowid
             WHERE sessions_fts MATCH ?1
             ORDER BY rank, s.last_accessed DESC LIMIT ?2"
        )
        .bind(query)
        .bind(limit as i32)
        .fetch_all(state.db.pool())
        .await?;
        
        let mut json_results = Vec::new();
        for row in results {
            let id: String = row.get("id");
            let name: Option<String> = row.get("name");
            let summary: Option<String> = row.get("summary");
            let workspace_path: Option<String> = row.get("workspace_path");
            let created_at: DateTime<Utc> = row.get("created_at");
            let last_accessed: DateTime<Utc> = row.get("last_accessed");
            let name_snippet: String = row.get("name_snippet");
            let summary_snippet: String = row.get("summary_snippet");
            
            json_results.push(json!({
                "source": "session",
                "id": id,
                "name": name,
                "summary": summary,
                "workspace_path": workspace_path,
                "name_snippet": name_snippet,
                "summary_snippet": summary_snippet,
                "created_at": created_at.to_rfc3339(),
                "last_accessed": last_accessed.to_rfc3339(),
                "relevance_score": 0.7,
                "match_type": "keyword"
            }));
        }
        
        Ok(json_results)
    }
    
    async fn get_details(&self, source: KnowledgeSource, id: &str, state: &KnowledgeAgentState) -> Result<serde_json::Value> {
        match source {
            KnowledgeSource::Memory => {
                let row = sqlx::query(
                    "SELECT * FROM memories WHERE id = ?1"
                )
                .bind(id)
                .fetch_one(state.db.pool())
                .await?;
                
                Ok(json!({
                    "source": "memory",
                    "id": row.get::<String, _>("id"),
                    "key": row.get::<String, _>("key"),
                    "content": row.get::<String, _>("content"),
                    "created_at": row.get::<DateTime<Utc>, _>("created_at").to_rfc3339(),
                    "accessed_at": row.get::<DateTime<Utc>, _>("accessed_at").to_rfc3339(),
                    "access_count": row.get::<i32, _>("access_count"),
                    "metadata": row.get::<Option<String>, _>("metadata")
                        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
                }))
            }
            KnowledgeSource::ChatHistory => {
                let row = sqlx::query(
                    "SELECT cm.*, s.name as session_name, s.workspace_path 
                     FROM chat_messages cm
                     JOIN sessions s ON cm.session_id = s.id
                     WHERE cm.id = ?1"
                )
                .bind(id)
                .fetch_one(state.db.pool())
                .await?;
                
                Ok(json!({
                    "source": "chat_history",
                    "id": row.get::<String, _>("id"),
                    "session_id": row.get::<String, _>("session_id"),
                    "session_name": row.get::<Option<String>, _>("session_name"),
                    "workspace_path": row.get::<Option<String>, _>("workspace_path"),
                    "role": row.get::<String, _>("role"),
                    "content": row.get::<Option<String>, _>("content"),
                    "tool_calls": row.get::<Option<String>, _>("tool_calls")
                        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok()),
                    "created_at": row.get::<DateTime<Utc>, _>("created_at").to_rfc3339()
                }))
            }
            KnowledgeSource::Todo => {
                let row = sqlx::query(
                    "SELECT t.*, s.name as session_name, s.workspace_path 
                     FROM todos t
                     JOIN sessions s ON t.session_id = s.id
                     WHERE t.id = ?1"
                )
                .bind(id)
                .fetch_one(state.db.pool())
                .await?;
                
                Ok(json!({
                    "source": "todo",
                    "id": row.get::<String, _>("id"),
                    "session_id": row.get::<String, _>("session_id"),
                    "session_name": row.get::<Option<String>, _>("session_name"),
                    "workspace_path": row.get::<Option<String>, _>("workspace_path"),
                    "content": row.get::<String, _>("content"),
                    "status": row.get::<String, _>("status"),
                    "priority": row.get::<String, _>("priority"),
                    "created_at": row.get::<DateTime<Utc>, _>("created_at").to_rfc3339(),
                    "updated_at": row.get::<DateTime<Utc>, _>("updated_at").to_rfc3339()
                }))
            }
            KnowledgeSource::Session => {
                let row = sqlx::query(
                    "SELECT * FROM sessions WHERE id = ?1"
                )
                .bind(id)
                .fetch_one(state.db.pool())
                .await?;
                
                // Get message count for this session
                let message_count: i64 = sqlx::query_scalar(
                    "SELECT COUNT(*) FROM chat_messages WHERE session_id = ?1"
                )
                .bind(id)
                .fetch_one(state.db.pool())
                .await?;
                
                // Get todo count for this session
                let todo_count: i64 = sqlx::query_scalar(
                    "SELECT COUNT(*) FROM todos WHERE session_id = ?1"
                )
                .bind(id)
                .fetch_one(state.db.pool())
                .await?;
                
                Ok(json!({
                    "source": "session",
                    "id": row.get::<String, _>("id"),
                    "name": row.get::<Option<String>, _>("name"),
                    "summary": row.get::<Option<String>, _>("summary"),
                    "workspace_path": row.get::<Option<String>, _>("workspace_path"),
                    "created_at": row.get::<DateTime<Utc>, _>("created_at").to_rfc3339(),
                    "last_accessed": row.get::<DateTime<Utc>, _>("last_accessed").to_rfc3339(),
                    "updated_at": row.get::<DateTime<Utc>, _>("updated_at").to_rfc3339(),
                    "metadata": row.get::<Option<String>, _>("metadata")
                        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok()),
                    "statistics": {
                        "message_count": message_count,
                        "todo_count": todo_count
                    }
                }))
            }
            KnowledgeSource::All => {
                Err(anyhow::anyhow!("Cannot get details for 'All' source type"))
            }
        }
    }
}

impl Actor for KnowledgeAgentActor {
    type Msg = ToolMessage;
    type State = KnowledgeAgentState;
    type Arguments = Config;
    
    async fn pre_start(
        &self,
        _myself: ActorRef<Self::Msg>,
        _config: Self::Arguments,
    ) -> Result<Self::State, ActorProcessingErr> {
        Ok(KnowledgeAgentState {
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
                let action: KnowledgeAction = match serde_json::from_value(params) {
                    Ok(a) => a,
                    Err(e) => {
                        chat_ref.send_message(ChatMessage::ToolResult {
                            id,
                            result: format!("Error parsing parameters: {}", e),
                        })?;
                        return Ok(());
                    }
                };
                
                let result = match action {
                    KnowledgeAction::Search { query, limit, source_filter, time_filter } => {
                        match self.search_knowledge(&query, limit, source_filter, time_filter, state).await {
                            Ok(results) => serde_json::to_string_pretty(&results)
                                .unwrap_or_else(|_| "Failed to format results".to_string()),
                            Err(e) => format!("Error searching knowledge: {}", e),
                        }
                    }
                    KnowledgeAction::GetDetails { source, id: item_id } => {
                        match self.get_details(source, &item_id, state).await {
                            Ok(details) => serde_json::to_string_pretty(&details)
                                .unwrap_or_else(|_| "Failed to format details".to_string()),
                            Err(e) => format!("Error getting details: {}", e),
                        }
                    }
                    KnowledgeAction::Analyze { topic, depth: _ } => {
                        // For now, perform a comprehensive search and return results
                        match self.search_knowledge(&topic, 30, None, None, state).await {
                            Ok(results) => {
                                format!("Analysis of '{}': {}", topic, serde_json::to_string_pretty(&results)
                                    .unwrap_or_else(|_| "Failed to format analysis".to_string()))
                            }
                            Err(e) => format!("Error analyzing topic: {}", e),
                        }
                    }
                    KnowledgeAction::Synthesize { topic, include_examples: _ } => {
                        // For now, perform a search and return a summary
                        match self.search_knowledge(&topic, 20, None, None, state).await {
                            Ok(results) => {
                                format!("Knowledge synthesis for '{}': {}", topic, serde_json::to_string_pretty(&results)
                                    .unwrap_or_else(|_| "Failed to format synthesis".to_string()))
                            }
                            Err(e) => format!("Error synthesizing knowledge: {}", e),
                        }
                    }
                };
                
                chat_ref.send_message(ChatMessage::ToolResult { id, result })?;
            }
            ToolMessage::Cancel { .. } => {
                // Knowledge agent doesn't support cancellation yet
                tracing::debug!("Cancel request received but not implemented");
            }
            ToolMessage::StreamUpdate { .. } => {
                // Knowledge agent doesn't support streaming yet
                tracing::debug!("Stream update received but not implemented");
            }
        }
        Ok(())
    }
}

