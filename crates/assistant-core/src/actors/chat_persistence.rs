use ractor::{Actor, ActorRef, ActorProcessingErr};
use chrono::{DateTime, Utc};
use uuid::Uuid;
use std::time::Duration;
use anyhow::Result;
use sqlx::Row;

use crate::config::Config;
use crate::actors::client::ClientMessage;
use crate::openai_compat::{ChatMessage as OpenAIMessage, UserContent};
use crate::persistence::database::Database;
use crate::embeddings::client::OpenAIEmbeddingClient;
use crate::embeddings::EmbeddingClient;

/// Messages for the ChatPersistenceActor
#[derive(Debug, Clone)]
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
}

/// Actor responsible for persisting chat messages and maintaining chat metadata
pub struct ChatPersistenceActor {
    config: Config,
    database: Database,
    embedding_client: Option<OpenAIEmbeddingClient>,
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
        let myself_clone = myself.clone();
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
                
                // Persist the message
                match self.persist_message(&session_id, "user", Some(&prompt), None).await {
                    Ok(_) => tracing::info!("Successfully persisted user prompt"),
                    Err(e) => tracing::error!("Failed to persist user prompt: {}", e),
                }
                
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
                tracing::info!("Persisting assistant response for session {}", session_id);
                
                // Persist the message
                match self.persist_message(&session_id, "assistant", Some(&response), None).await {
                    Ok(_) => tracing::info!("Successfully persisted assistant response"),
                    Err(e) => tracing::error!("Failed to persist assistant response: {}", e),
                }
            }
            
            ChatPersistenceMessage::PersistToolInteraction { id: _, session_id, tool_name, parameters, result } => {
                tracing::debug!("Persisting tool interaction for session {}", session_id);
                
                // Create tool call structure
                let tool_calls = Some(serde_json::json!({
                    "tool": tool_name,
                    "parameters": parameters,
                    "result": result,
                }));
                
                // Persist as a special message type
                self.persist_message(&session_id, "tool", None, tool_calls).await?;
            }
            
            ChatPersistenceMessage::GenerateChatName { session_id, first_message } => {
                tracing::info!("Generating chat name for session {}", session_id);
                
                // Generate name asynchronously
                let myself_clone = myself.clone();
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
            if model_config.provider == "openai" {
                if let Some(api_key) = &model_config.api_key {
                    let base_url = model_config.base_url.as_ref()
                        .unwrap_or(&"https://api.openai.com/v1".to_string())
                        .clone();
                    let model = crate::embeddings::client::OpenAIEmbeddingModel::Custom(model_config.model.clone());
                    Some(OpenAIEmbeddingClient::new(api_key.clone(), base_url, model))
                } else {
                    tracing::warn!("OpenAI embeddings configured but no API key provided");
                    None
                }
            } else {
                tracing::warn!("Non-OpenAI embedding provider not yet supported in chat persistence");
                None
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
        // Ensure session exists first
        self.ensure_session_exists(session_id).await?;
        
        let id = Uuid::new_v4().to_string();
        let now = Utc::now();
        
        // Generate embedding for content if available
        let embedding_bytes = if let (Some(content), Some(client)) = (content, &self.embedding_client) {
            match client.embed(content).await {
                Ok(embedding) => {
                    Some(embedding.iter().flat_map(|f| f.to_le_bytes()).collect::<Vec<u8>>())
                }
                Err(e) => {
                    tracing::warn!("Failed to generate embedding for message: {}", e);
                    None
                }
            }
        } else {
            None
        };
        
        // Insert message
        sqlx::query(
            r#"
            INSERT INTO chat_messages (id, session_id, role, content, tool_calls, embedding, created_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
            "#,
        )
        .bind(&id)
        .bind(session_id)
        .bind(role)
        .bind(content)
        .bind(tool_calls.and_then(|v| serde_json::to_string(&v).ok()))
        .bind(embedding_bytes)
        .bind(&now)
        .execute(self.database.pool())
        .await?;
        
        // Update session last_accessed
        sqlx::query(
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
        let name = if let Some(client) = client_ref {
            // Call LLM to generate name
            let prompt = format!(
                "Generate a short, descriptive title (max 50 characters) for a conversation that starts with: \"{}\"", 
                first_message
            );
            
            let messages = vec![
                OpenAIMessage::System {
                    content: "You are a helpful assistant that generates concise, descriptive titles for conversations.".to_string(),
                    name: None,
                },
                OpenAIMessage::User {
                    content: UserContent::Text(prompt),
                    name: None,
                },
            ];
            
            // TODO: Actually call the LLM through the client
            // For now, use a simple truncation
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
        embedding_client: Option<OpenAIEmbeddingClient>,
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
        
        // Build conversation text
        let mut conversation = String::new();
        for row in rows.iter().rev() {
            let role: String = row.get(0);
            let content: String = row.get(1);
            conversation.push_str(&format!("{}: {}\n", role, content));
        }
        
        let summary = if let Some(client) = client_ref {
            // TODO: Call LLM to generate summary
            // For now, use a simple truncation
            format!("Summary of {} messages", rows.len())
        } else {
            format!("Summary of {} messages", rows.len())
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
        
        tracing::info!("Generated summary for session {}", session_id);
        Ok(())
    }
}