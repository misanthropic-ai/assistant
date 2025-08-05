use ractor::{Actor, ActorRef, ActorProcessingErr};
use chrono::{DateTime, Utc};
use uuid::Uuid;
use std::time::Duration;
use std::collections::HashMap;
use std::sync::Arc;
use anyhow::Result;
use sqlx::Row;

use crate::config::Config;
use crate::actors::client::ClientMessage;
use crate::openai_compat::{ChatMessage as OpenAIMessage, UserContent};
use crate::persistence::database::Database;
use crate::embeddings::{
    client::OpenAIEmbeddingClient,
    ollama::{OllamaEmbeddingClient, OllamaEmbeddingModel},
    EmbeddingClient,
};

/// Represents a database operation that needs to be tracked
#[derive(Debug, Clone)]
#[allow(dead_code)]
enum DatabaseOperation {
    PersistMessage {
        session_id: String,
        role: String,
        content: Option<String>,
        tool_calls: Option<serde_json::Value>,
    },
    GenerateName {
        session_id: String,
        first_message: String,
    },
    Summarize {
        session_id: String,
    },
}

/// Messages for the ChatPersistenceActor
#[derive(Debug)]
pub enum ChatPersistenceMessage {
    /// Persist a user prompt
    PersistUserPrompt {
        id: Uuid,
        session_id: String,
        prompt: String,
    },
    /// Persist an assistant response
    PersistAssistantResponse {
        id: Uuid,
        session_id: String,
        response: String,
    },
    /// Persist a tool call/result
    PersistToolInteraction {
        id: Uuid,
        session_id: String,
        tool_name: String,
        parameters: Option<serde_json::Value>,
        result: Option<String>,
    },
    /// Generate a name for the chat (triggered on first message)
    GenerateChatName {
        session_id: String,
        first_message: String,
    },
    /// Summarize the chat
    SummarizeChat {
        session_id: String,
    },
    /// Get pending operations count
    GetPendingCount {
        reply_to: tokio::sync::oneshot::Sender<usize>,
    },
    /// Wait for all operations to complete
    WaitForCompletion {
        reply_to: tokio::sync::oneshot::Sender<()>,
    },
    /// Internal: Operation completed
    OperationComplete {
        operation_id: Uuid,
        success: bool,
        error: Option<String>,
    },
}

/// Actor responsible for persisting chat messages and maintaining chat metadata
pub struct ChatPersistenceActor {
    #[allow(dead_code)]
    config: Config,
    database: Database,
    embedding_client: Option<Arc<dyn EmbeddingClient + Send + Sync>>,
    client_ref: Option<ActorRef<ClientMessage>>,
}

/// State for the chat persistence actor
pub struct ChatPersistenceState {
    /// Sessions that have had their names generated
    named_sessions: std::collections::HashSet<String>,
    /// Last summarization time for each session
    last_summarized: std::collections::HashMap<String, DateTime<Utc>>,
    /// Summarization interval (default: 10 minutes)
    summarization_interval: Duration,
    /// Queue of pending database operations
    pending_operations: HashMap<Uuid, DatabaseOperation>,
    /// Notify when all operations complete
    completion_notifiers: Vec<tokio::sync::oneshot::Sender<()>>,
}

impl Actor for ChatPersistenceActor {
    type Msg = ChatPersistenceMessage;
    type State = ChatPersistenceState;
    type Arguments = ();
    
    async fn pre_start(
        &self,
        myself: ActorRef<Self::Msg>,
        _args: Self::Arguments,
    ) -> Result<Self::State, ActorProcessingErr> {
        tracing::info!("ChatPersistenceActor starting");
        
        // Schedule periodic summarization check (every 5 minutes)
        let _myself_clone = myself.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(300));
            loop {
                interval.tick().await;
                // Check all active sessions for summarization needs
                // This would need to be implemented with proper session tracking
                tracing::debug!("Periodic summarization check");
            }
        });
        
        Ok(ChatPersistenceState {
            named_sessions: std::collections::HashSet::new(),
            last_summarized: std::collections::HashMap::new(),
            summarization_interval: Duration::from_secs(600), // 10 minutes
            pending_operations: HashMap::new(),
            completion_notifiers: Vec::new(),
        })
    }
    
    async fn handle(
        &self,
        myself: ActorRef<Self::Msg>,
        msg: Self::Msg,
        state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        match msg {
            ChatPersistenceMessage::PersistUserPrompt { id: _, session_id, prompt } => {
                tracing::info!("Persisting user prompt for session {}: {}", session_id, prompt);
                
                // Create operation and add to queue
                let operation_id = Uuid::new_v4();
                let operation = DatabaseOperation::PersistMessage {
                    session_id: session_id.clone(),
                    role: "user".to_string(),
                    content: Some(prompt.clone()),
                    tool_calls: None,
                };
                state.pending_operations.insert(operation_id, operation.clone());
                
                // Spawn task to perform the operation
                let database = self.database.clone();
                let embedding_client = self.embedding_client.clone();
                let myself_clone = myself.clone();
                
                tokio::spawn(async move {
                    // Create actor instance for the operation
                    let actor = ChatPersistenceActor {
                        config: Default::default(),
                        database,
                        embedding_client,
                        client_ref: None,
                    };
                    
                    // Perform the operation
                    let result = match operation {
                        DatabaseOperation::PersistMessage { session_id, role, content, tool_calls } => {
                            actor.persist_message(&session_id, &role, content.as_deref(), tool_calls).await
                        }
                        _ => unreachable!(),
                    };
                    
                    // Send completion message
                    let _ = myself_clone.send_message(ChatPersistenceMessage::OperationComplete {
                        operation_id,
                        success: result.is_ok(),
                        error: result.err().map(|e| e.to_string()),
                    });
                });
                
                // Generate chat name on first message
                if !state.named_sessions.contains(&session_id) {
                    state.named_sessions.insert(session_id.clone());
                    myself.send_message(ChatPersistenceMessage::GenerateChatName {
                        session_id: session_id.clone(),
                        first_message: prompt,
                    })?;
                }
                
                // Check if we need to summarize
                if self.should_summarize(&session_id, state) {
                    myself.send_message(ChatPersistenceMessage::SummarizeChat { 
                        session_id: session_id.clone() 
                    })?;
                }
            }
            
            ChatPersistenceMessage::PersistAssistantResponse { id: _, session_id, response } => {
                tracing::info!("Persisting assistant response for session {}: {}", session_id, response);
                
                // Create operation and add to queue
                let operation_id = Uuid::new_v4();
                let operation = DatabaseOperation::PersistMessage {
                    session_id: session_id.clone(),
                    role: "assistant".to_string(),
                    content: Some(response),
                    tool_calls: None,
                };
                state.pending_operations.insert(operation_id, operation.clone());
                
                tracing::debug!("Added assistant response to queue with operation_id: {}", operation_id);
                
                // Spawn task to perform the operation
                let database = self.database.clone();
                let embedding_client = self.embedding_client.clone();
                let myself_clone = myself.clone();
                
                tokio::spawn(async move {
                    tracing::debug!("Starting async operation for assistant response {}", operation_id);
                    
                    // Create actor instance for the operation
                    let actor = ChatPersistenceActor {
                        config: Default::default(),
                        database,
                        embedding_client,
                        client_ref: None,
                    };
                    
                    // Perform the operation
                    let result = match operation {
                        DatabaseOperation::PersistMessage { session_id, role, content, tool_calls } => {
                            actor.persist_message(&session_id, &role, content.as_deref(), tool_calls).await
                        }
                        _ => unreachable!(),
                    };
                    
                    // Send completion message
                    let _ = myself_clone.send_message(ChatPersistenceMessage::OperationComplete {
                        operation_id,
                        success: result.is_ok(),
                        error: result.err().map(|e| e.to_string()),
                    });
                });
            }
            
            ChatPersistenceMessage::PersistToolInteraction { id: _, session_id, tool_name, parameters, result } => {
                tracing::debug!("Persisting tool interaction for session {}", session_id);
                
                // Create tool call structure
                let tool_calls = Some(serde_json::json!({
                    "tool": tool_name,
                    "parameters": parameters,
                    "result": result,
                }));
                
                // Create operation and add to queue
                let operation_id = Uuid::new_v4();
                let operation = DatabaseOperation::PersistMessage {
                    session_id: session_id.clone(),
                    role: "tool".to_string(),
                    content: None,
                    tool_calls,
                };
                state.pending_operations.insert(operation_id, operation.clone());
                
                // Spawn task to perform the operation
                let database = self.database.clone();
                let embedding_client = self.embedding_client.clone();
                let myself_clone = myself.clone();
                
                tokio::spawn(async move {
                    // Create actor instance for the operation
                    let actor = ChatPersistenceActor {
                        config: Default::default(),
                        database,
                        embedding_client,
                        client_ref: None,
                    };
                    
                    // Perform the operation
                    let result = match operation {
                        DatabaseOperation::PersistMessage { session_id, role, content, tool_calls } => {
                            actor.persist_message(&session_id, &role, content.as_deref(), tool_calls).await
                        }
                        _ => unreachable!(),
                    };
                    
                    // Send completion message
                    let _ = myself_clone.send_message(ChatPersistenceMessage::OperationComplete {
                        operation_id,
                        success: result.is_ok(),
                        error: result.err().map(|e| e.to_string()),
                    });
                });
            }
            
            ChatPersistenceMessage::GenerateChatName { session_id, first_message } => {
                tracing::info!("Generating chat name for session {}", session_id);
                
                // Generate name asynchronously
                let _myself_clone = myself.clone();
                let client_ref = self.client_ref.clone();
                let database = self.database.clone();
                
                tokio::spawn(async move {
                    if let Err(e) = Self::generate_and_save_chat_name(
                        &session_id,
                        &first_message,
                        client_ref,
                        database,
                    ).await {
                        tracing::error!("Failed to generate chat name: {}", e);
                    }
                });
            }
            
            ChatPersistenceMessage::SummarizeChat { session_id } => {
                tracing::info!("Summarizing chat for session {}", session_id);
                
                // Update last summarized time
                state.last_summarized.insert(session_id.clone(), Utc::now());
                
                // Summarize asynchronously
                let client_ref = self.client_ref.clone();
                let database = self.database.clone();
                let embedding_client = self.embedding_client.clone();
                
                tokio::spawn(async move {
                    if let Err(e) = Self::generate_and_save_summary(
                        &session_id,
                        client_ref,
                        database,
                        embedding_client,
                    ).await {
                        tracing::error!("Failed to summarize chat: {}", e);
                    }
                });
            }
            
            ChatPersistenceMessage::GetPendingCount { reply_to } => {
                let count = state.pending_operations.len();
                tracing::debug!("Pending operations count: {}", count);
                let _ = reply_to.send(count);
            }
            
            ChatPersistenceMessage::WaitForCompletion { reply_to } => {
                if state.pending_operations.is_empty() {
                    // No pending operations, reply immediately
                    tracing::debug!("No pending operations, sending completion immediately");
                    let _ = reply_to.send(());
                } else {
                    // Store the notifier to be called when operations complete
                    tracing::debug!("Waiting for {} pending operations", state.pending_operations.len());
                    state.completion_notifiers.push(reply_to);
                }
            }
            
            ChatPersistenceMessage::OperationComplete { operation_id, success, error } => {
                tracing::debug!("Operation {} completed: success={}, error={:?}", operation_id, success, error);
                
                // Remove from pending operations
                if let Some(operation) = state.pending_operations.remove(&operation_id) {
                    if !success {
                        tracing::error!("Database operation failed: {:?} - Error: {:?}", operation, error);
                    } else {
                        tracing::info!("Database operation completed successfully: {:?}", operation);
                    }
                    
                    // If queue is now empty, notify all waiters
                    if state.pending_operations.is_empty() {
                        tracing::debug!("All operations complete, notifying {} waiters", state.completion_notifiers.len());
                        for notifier in state.completion_notifiers.drain(..) {
                            let _ = notifier.send(());
                        }
                    }
                } else {
                    tracing::warn!("Received completion for unknown operation: {}", operation_id);
                }
            }
        }
        
        Ok(())
    }
}

impl ChatPersistenceActor {
    pub async fn new(config: Config) -> Result<Self> {
        // Get database path from config
        let db_path = config.session.database_path.clone()
            .unwrap_or_else(|| {
                dirs::home_dir()
                    .unwrap_or_else(|| std::path::PathBuf::from("/tmp"))
                    .join(".assistant")
                    .join("assistant.db")
            });
        
        let database = Database::new(&db_path).await?;
        
        // Create embedding client if configured
        let embedding_client = if let Some(model_config) = config.embeddings.models.get(&config.embeddings.default_model) {
            match model_config.provider.as_str() {
                "openai" => {
                    if let Some(api_key) = &model_config.api_key {
                        let base_url = model_config.base_url.as_ref()
                            .unwrap_or(&"https://api.openai.com/v1".to_string())
                            .clone();
                        let model = crate::embeddings::client::OpenAIEmbeddingModel::Custom(model_config.model.clone());
                        Some(Arc::new(OpenAIEmbeddingClient::new(api_key.clone(), base_url, model)) as Arc<dyn EmbeddingClient + Send + Sync>)
                    } else {
                        tracing::warn!("OpenAI embeddings configured but no API key provided");
                        None
                    }
                }
                "ollama" => {
                    let base_url = model_config.base_url.as_ref()
                        .unwrap_or(&"http://localhost:11434".to_string())
                        .clone();
                    let model = match model_config.model.as_str() {
                        "mxbai-embed-large" => OllamaEmbeddingModel::MxbaiEmbedLarge,
                        other => OllamaEmbeddingModel::Custom(other.to_string()),
                    };
                    Some(Arc::new(OllamaEmbeddingClient::new(base_url, model)) as Arc<dyn EmbeddingClient + Send + Sync>)
                }
                other => {
                    tracing::warn!("Unsupported embedding provider: {}", other);
                    None
                }
            }
        } else {
            None
        };
        
        Ok(Self {
            config,
            database,
            embedding_client,
            client_ref: None,
        })
    }
    
    pub fn with_client_ref(mut self, client_ref: ActorRef<ClientMessage>) -> Self {
        self.client_ref = Some(client_ref);
        self
    }
    
    async fn ensure_session_exists(&self, session_id: &str) -> Result<()> {
        let now = Utc::now();
        
        // Use INSERT OR IGNORE to handle concurrent creation attempts
        sqlx::query(
            r#"
            INSERT OR IGNORE INTO sessions (id, created_at, last_accessed, updated_at)
            VALUES (?1, ?2, ?3, ?4)
            "#,
        )
        .bind(session_id)
        .bind(&now)
        .bind(&now)
        .bind(&now)
        .execute(self.database.pool())
        .await?;
        
        Ok(())
    }
    
    async fn persist_message(
        &self,
        session_id: &str,
        role: &str,
        content: Option<&str>,
        tool_calls: Option<serde_json::Value>,
    ) -> Result<()> {
        tracing::debug!(
            "persist_message called: session_id={}, role={}, content={:?}, has_tool_calls={}", 
            session_id, role, content, tool_calls.is_some()
        );
        
        // Ensure session exists first
        self.ensure_session_exists(session_id).await?;
        
        let id = Uuid::new_v4().to_string();
        let now = Utc::now();
        
        // Generate embedding for content if available
        tracing::debug!("About to generate embedding for role={}", role);
        let embedding_bytes = if let (Some(content), Some(client)) = (content, &self.embedding_client) {
            tracing::debug!("Generating embedding for {} message with {} chars", role, content.len());
            match client.embed(content).await {
                Ok(embedding) => {
                    tracing::debug!("Successfully generated embedding with {} dimensions", embedding.len());
                    Some(embedding.iter().flat_map(|f| f.to_le_bytes()).collect::<Vec<u8>>())
                }
                Err(e) => {
                    tracing::error!("Failed to generate embedding for {} message: {:?}", role, e);
                    None
                }
            }
        } else {
            tracing::debug!("Skipping embedding - content or client not available");
            None
        };
        
        tracing::debug!(
            "Inserting message: id={}, session_id={}, role={}, content_len={}, has_embedding={}", 
            id, session_id, role, 
            content.map(|c| c.len()).unwrap_or(0),
            embedding_bytes.is_some()
        );
        
        // Insert message
        tracing::debug!("Preparing SQL insert with parameters");
        let tool_calls_str = tool_calls.and_then(|v| {
            match serde_json::to_string(&v) {
                Ok(s) => {
                    tracing::debug!("Serialized tool_calls: {}", s);
                    Some(s)
                }
                Err(e) => {
                    tracing::error!("Failed to serialize tool_calls: {:?}", e);
                    None
                }
            }
        });
        
        let query = sqlx::query(
            r#"
            INSERT INTO chat_messages (id, session_id, role, content, tool_calls, embedding, created_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
            "#,
        )
        .bind(&id)
        .bind(session_id)
        .bind(role)
        .bind(content)
        .bind(tool_calls_str)
        .bind(embedding_bytes.as_ref().map(|b| b.as_slice()))
        .bind(&now);
        
        tracing::debug!("Executing SQL insert");
        let result = match query.execute(self.database.pool()).await {
            Ok(result) => {
                tracing::debug!("SQL insert succeeded");
                result
            }
            Err(e) => {
                tracing::error!("SQL INSERT failed: {:?}", e);
                return Err(anyhow::anyhow!("Failed to insert message: {}", e));
            }
        };
        
        let rows_affected = result.rows_affected();
        tracing::info!(
            "Message insert result: rows_affected={} for role={} in session={}", 
            rows_affected, role, session_id
        );
        
        if rows_affected == 0 {
            tracing::error!("No rows inserted for message!");
            return Err(anyhow::anyhow!("Failed to insert message - no rows affected"));
        }
        
        // Update session last_accessed
        let session_result = sqlx::query(
            r#"
            UPDATE sessions
            SET last_accessed = ?1, updated_at = ?1
            WHERE id = ?2
            "#,
        )
        .bind(&now)
        .bind(session_id)
        .execute(self.database.pool())
        .await?;
        
        tracing::debug!(
            "Session update result: rows_affected={} for session={}", 
            session_result.rows_affected(), session_id
        );
        
        Ok(())
    }
    
    fn should_summarize(
        &self,
        session_id: &str,
        state: &ChatPersistenceState,
    ) -> bool {
        if let Some(last_time) = state.last_summarized.get(session_id) {
            let elapsed = Utc::now().signed_duration_since(*last_time);
            elapsed > chrono::Duration::from_std(state.summarization_interval).unwrap_or(chrono::Duration::minutes(10))
        } else {
            // Never summarized, check if we have enough messages
            true // For now, always summarize on first check
        }
    }
    
    async fn generate_and_save_chat_name(
        session_id: &str,
        first_message: &str,
        client_ref: Option<ActorRef<ClientMessage>>,
        database: Database,
    ) -> Result<()> {
        let name = if let Some(_client) = client_ref {
            // Call LLM to generate name
            let _prompt = format!(
                "Generate a short, descriptive title (max 50 characters) for a conversation that starts with: \"{}\"", 
                first_message
            );
            
            let _messages = vec![
                OpenAIMessage::System {
                    content: "You are a helpful assistant that generates concise, descriptive titles for conversations. Respond with ONLY the title, nothing else.".to_string(),
                    name: None,
                },
                OpenAIMessage::User {
                    content: UserContent::Text(_prompt),
                    name: None,
                },
            ];
            
            // For now, we can't easily get responses from the client actor
            // as it communicates back via the chat actor. We'll use a simple approach
            // TODO: Implement a proper request-response pattern for the client actor
            first_message.chars().take(50).collect::<String>()
        } else {
            // Fallback: use truncated first message
            first_message.chars().take(50).collect::<String>()
        };
        
        // Update session with name
        sqlx::query(
            r#"
            UPDATE sessions
            SET name = ?1, updated_at = ?2
            WHERE id = ?3
            "#,
        )
        .bind(&name)
        .bind(Utc::now())
        .bind(session_id)
        .execute(database.pool())
        .await?;
        
        tracing::info!("Generated chat name for session {}: {}", session_id, name);
        Ok(())
    }
    
    async fn generate_and_save_summary(
        session_id: &str,
        client_ref: Option<ActorRef<ClientMessage>>,
        database: Database,
        embedding_client: Option<Arc<dyn EmbeddingClient + Send + Sync>>,
    ) -> Result<()> {
        // Fetch recent messages
        let rows = sqlx::query(
            r#"
            SELECT role, content
            FROM chat_messages
            WHERE session_id = ?1 AND content IS NOT NULL
            ORDER BY created_at DESC
            LIMIT 50
            "#,
        )
        .bind(session_id)
        .fetch_all(database.pool())
        .await?;
        
        if rows.is_empty() {
            tracing::info!("No messages to summarize for session {}", session_id);
            return Ok(());
        }
        
        // Build conversation text
        let mut conversation = String::new();
        for row in rows.iter().rev() {
            let role: String = row.get(0);
            let content: String = row.get(1);
            conversation.push_str(&format!("{}: {}\n", role, content));
        }
        
        let summary = if let Some(_client) = client_ref {
            // Call LLM to generate summary
            let _prompt = format!(
                "Please provide a concise summary (2-3 sentences) of the following conversation:\n\n{}", 
                conversation
            );
            
            let _messages = vec![
                OpenAIMessage::System {
                    content: "You are a helpful assistant that summarizes conversations. Provide clear, concise summaries that capture the key topics and outcomes. Respond with ONLY the summary, nothing else.".to_string(),
                    name: None,
                },
                OpenAIMessage::User {
                    content: UserContent::Text(_prompt),
                    name: None,
                },
            ];
            
            // For now, we can't easily get responses from the client actor
            // as it communicates back via the chat actor. We'll use a simple approach
            // TODO: Implement a proper request-response pattern for the client actor
            let key_topics = if rows.len() > 5 {
                "multiple topics discussed"
            } else {
                "initial discussion"
            };
            format!("Summary of {} messages: {}", rows.len(), key_topics)
        } else {
            format!("Summary of {} messages in the conversation", rows.len())
        };
        
        // Generate embedding for summary
        let embedding_bytes = if let Some(client) = &embedding_client {
            match client.embed(&summary).await {
                Ok(embedding) => {
                    Some(embedding.iter().flat_map(|f| f.to_le_bytes()).collect::<Vec<u8>>())
                }
                Err(e) => {
                    tracing::warn!("Failed to generate embedding for summary: {}", e);
                    None
                }
            }
        } else {
            None
        };
        
        // Update session with summary
        sqlx::query(
            r#"
            UPDATE sessions
            SET summary = ?1, summary_embedding = ?2, updated_at = ?3
            WHERE id = ?4
            "#,
        )
        .bind(&summary)
        .bind(embedding_bytes)
        .bind(Utc::now())
        .bind(session_id)
        .execute(database.pool())
        .await?;
        
        tracing::info!("Generated summary for session {}: {}", session_id, summary);
        Ok(())
    }
}