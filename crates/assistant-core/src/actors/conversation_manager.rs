use ractor::{Actor, ActorRef, ActorProcessingErr};
use anyhow::Result;
use std::collections::HashMap;

use crate::persistence::database::Database;
use crate::persistence::schema::{SessionSummary, ChatMessageRecord};
use crate::openai_compat::ChatMessage as OpenAIMessage;
use crate::messages::ChatMessage;

/// Messages for the ConversationManagerActor
#[derive(Debug)]
pub enum ConversationManagerMessage {
    /// List all conversations
    ListConversations {
        reply_to: tokio::sync::oneshot::Sender<Result<Vec<SessionSummary>>>,
        limit: i64,
        offset: i64,
    },
    /// Search conversations
    SearchConversations {
        reply_to: tokio::sync::oneshot::Sender<Result<Vec<SessionSummary>>>,
        query: String,
        limit: i64,
    },
    /// Load a conversation
    LoadConversation {
        reply_to: tokio::sync::oneshot::Sender<Result<Vec<OpenAIMessage>>>,
        session_id: String,
    },
    /// Create a new conversation
    CreateConversation {
        reply_to: tokio::sync::oneshot::Sender<Result<String>>,
        workspace_path: Option<String>,
    },
    /// Rename a conversation
    RenameConversation {
        reply_to: tokio::sync::oneshot::Sender<Result<()>>,
        session_id: String,
        new_name: String,
    },
    /// Delete a conversation
    DeleteConversation {
        reply_to: tokio::sync::oneshot::Sender<Result<()>>,
        session_id: String,
    },
    /// Switch to a different conversation
    SwitchConversation {
        session_id: String,
        chat_ref: ActorRef<ChatMessage>,
    },
    /// Get the current session ID
    GetCurrentSession {
        reply_to: tokio::sync::oneshot::Sender<Option<String>>,
    },
}

/// Actor responsible for managing conversations
pub struct ConversationManagerActor {
    database: Database,
}

/// State for the conversation manager actor
pub struct ConversationManagerState {
    /// Current active session ID
    current_session_id: Option<String>,
    /// Cache of loaded conversations
    conversation_cache: HashMap<String, Vec<OpenAIMessage>>,
}

impl Actor for ConversationManagerActor {
    type Msg = ConversationManagerMessage;
    type State = ConversationManagerState;
    type Arguments = ();
    
    async fn pre_start(
        &self,
        _myself: ActorRef<Self::Msg>,
        _args: Self::Arguments,
    ) -> Result<Self::State, ActorProcessingErr> {
        tracing::info!("ConversationManagerActor starting");
        
        Ok(ConversationManagerState {
            current_session_id: None,
            conversation_cache: HashMap::new(),
        })
    }
    
    async fn handle(
        &self,
        _myself: ActorRef<Self::Msg>,
        msg: Self::Msg,
        state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        match msg {
            ConversationManagerMessage::ListConversations { reply_to, limit, offset } => {
                let result = self.database.list_sessions(limit, offset).await;
                let _ = reply_to.send(result);
            }
            
            ConversationManagerMessage::SearchConversations { reply_to, query, limit } => {
                let result = self.database.search_sessions(&query, limit).await;
                let _ = reply_to.send(result);
            }
            
            ConversationManagerMessage::LoadConversation { reply_to, session_id } => {
                tracing::info!("LoadConversation requested for session: {}", session_id);
                
                // Check cache first
                if let Some(messages) = state.conversation_cache.get(&session_id) {
                    tracing::info!("Found {} messages in cache for session {}", messages.len(), session_id);
                    let _ = reply_to.send(Ok(messages.clone()));
                    return Ok(());
                }
                
                // Load from database
                match self.database.get_session_messages(&session_id, None, None).await {
                    Ok(records) => {
                        tracing::info!("Loaded {} records from database for session {}", records.len(), session_id);
                        let messages = self.convert_to_openai_messages(records);
                        tracing::info!("Converted to {} OpenAI messages for session {}", messages.len(), session_id);
                        
                        // Cache the messages
                        state.conversation_cache.insert(session_id.clone(), messages.clone());
                        let _ = reply_to.send(Ok(messages));
                    }
                    Err(e) => {
                        tracing::error!("Failed to load messages for session {}: {}", session_id, e);
                        let _ = reply_to.send(Err(e));
                    }
                }
            }
            
            ConversationManagerMessage::CreateConversation { reply_to, workspace_path } => {
                let result = self.database.create_session(workspace_path.as_deref()).await;
                if let Ok(ref session_id) = result {
                    state.current_session_id = Some(session_id.clone());
                }
                let _ = reply_to.send(result);
            }
            
            ConversationManagerMessage::RenameConversation { reply_to, session_id, new_name } => {
                let result = self.database.rename_session(&session_id, &new_name).await;
                let _ = reply_to.send(result);
            }
            
            ConversationManagerMessage::DeleteConversation { reply_to, session_id } => {
                // Remove from cache if present
                state.conversation_cache.remove(&session_id);
                
                // If deleting current session, clear it
                if state.current_session_id.as_ref() == Some(&session_id) {
                    state.current_session_id = None;
                }
                
                let result = self.database.delete_session(&session_id).await;
                let _ = reply_to.send(result);
            }
            
            ConversationManagerMessage::SwitchConversation { session_id, chat_ref } => {
                // Clear cache for the previous session to ensure fresh load next time
                if let Some(ref current_id) = state.current_session_id {
                    state.conversation_cache.remove(current_id);
                    tracing::info!("Cleared cache for previous session: {}", current_id);
                }
                
                state.current_session_id = Some(session_id.clone());
                
                // Load the conversation messages
                match self.database.get_session_messages(&session_id, None, None).await {
                    Ok(records) => {
                        let messages = self.convert_to_openai_messages(records);
                        
                        // Cache the messages
                        state.conversation_cache.insert(session_id.clone(), messages.clone());
                        
                        // Send a message to the chat actor to switch context
                        let _ = chat_ref.send_message(ChatMessage::SwitchSession {
                            session_id: session_id.clone(),
                            messages,
                        });
                    }
                    Err(e) => {
                        tracing::error!("Failed to load conversation {}: {}", session_id, e);
                    }
                }
            }
            
            ConversationManagerMessage::GetCurrentSession { reply_to } => {
                let _ = reply_to.send(state.current_session_id.clone());
            }
        }
        
        Ok(())
    }
}

impl ConversationManagerActor {
    pub async fn new(database: Database) -> Result<Self> {
        Ok(Self { database })
    }
    
    /// Convert database records to OpenAI messages
    fn convert_to_openai_messages(&self, records: Vec<ChatMessageRecord>) -> Vec<OpenAIMessage> {
        records.into_iter()
            .filter_map(|record| {
                match record.role.as_str() {
                    "user" => record.content.map(|content| OpenAIMessage::User {
                        content: crate::openai_compat::UserContent::Text(content),
                        name: None,
                    }),
                    "assistant" => record.content.map(|content| OpenAIMessage::Assistant {
                        content: Some(content),
                        name: None,
                        tool_calls: record.tool_calls.and_then(|tc| {
                            serde_json::from_value(tc).ok()
                        }),
                    }),
                    "system" => record.content.map(|content| OpenAIMessage::System {
                        content,
                        name: None,
                    }),
                    "tool" => {
                        // Parse tool messages from the stored JSON
                        if let Some(tool_data) = record.tool_calls {
                            if let Ok(data) = serde_json::from_value::<serde_json::Value>(tool_data) {
                                if let (Some(tool_id), Some(result)) = (
                                    data.get("tool").and_then(|t| t.as_str()),
                                    data.get("result").and_then(|r| r.as_str()),
                                ) {
                                    return Some(OpenAIMessage::Tool {
                                        content: result.to_string(),
                                        tool_call_id: tool_id.to_string(),
                                    });
                                }
                            }
                        }
                        None
                    }
                    _ => None,
                }
            })
            .collect()
    }
}