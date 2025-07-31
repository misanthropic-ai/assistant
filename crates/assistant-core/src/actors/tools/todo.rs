use ractor::{Actor, ActorRef, ActorProcessingErr};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use crate::config::Config;
use crate::messages::{ToolMessage, ChatMessage};
use anyhow::Result;
use sqlx::Row;
use uuid::Uuid;
use chrono::Utc;
use crate::persistence::database::Database;

pub struct TodoActor {
    config: Config,
    database: Database,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Todo {
    pub id: String,
    pub content: String,
    pub status: TodoStatus,
    pub priority: TodoPriority,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum TodoStatus {
    Pending,
    InProgress,
    Completed,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum TodoPriority {
    High,
    Medium,
    Low,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case")]
pub enum TodoOperation {
    List { 
        session_id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        status: Option<TodoStatus>,
        #[serde(skip_serializing_if = "Option::is_none")]
        priority: Option<TodoPriority> 
    },
    Add { 
        session_id: String,
        content: String,
        priority: TodoPriority 
    },
    Update { 
        session_id: String,
        id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        content: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        status: Option<TodoStatus>,
        #[serde(skip_serializing_if = "Option::is_none")]
        priority: Option<TodoPriority> 
    },
    Remove { 
        session_id: String,
        id: String 
    },
    Clear { 
        session_id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        status: Option<TodoStatus> 
    },
    Stats {
        session_id: String,
    },
}

pub struct TodoState {}

impl Actor for TodoActor {
    type Msg = ToolMessage;
    type State = TodoState;
    type Arguments = Config;
    
    async fn pre_start(
        &self,
        _myself: ActorRef<Self::Msg>,
        _config: Self::Arguments,
    ) -> Result<Self::State, ActorProcessingErr> {
        tracing::debug!("Todo actor starting");
        Ok(TodoState {})
    }
    
    async fn handle(
        &self,
        _myself: ActorRef<Self::Msg>,
        msg: Self::Msg,
        _state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        match msg {
            ToolMessage::Execute { id, params, chat_ref } => {
                tracing::info!("Todo tool execution with params: {:?}", params);
                
                // Extract session_id from params (default to "default")
                let session_id = params.get("session_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("default")
                    .to_string();
                    
                // Extract operation from params
                let operation_str = match params.get("operation").and_then(|v| v.as_str()) {
                    Some(op) => op,
                    None => {
                        chat_ref.send_message(ChatMessage::ToolResult {
                            id,
                            result: "Error: Missing 'operation' field".to_string(),
                        })?;
                        return Ok(());
                    }
                };
                
                // Build the operation based on the operation type
                let operation = match operation_str {
                    "list" => TodoOperation::List {
                        session_id: session_id.clone(),
                        status: params.get("status").and_then(|v| v.as_str()).and_then(|s| match s {
                            "pending" => Some(TodoStatus::Pending),
                            "in_progress" => Some(TodoStatus::InProgress),
                            "completed" => Some(TodoStatus::Completed),
                            _ => None,
                        }),
                        priority: params.get("priority").and_then(|v| v.as_str()).and_then(|s| match s {
                            "high" => Some(TodoPriority::High),
                            "medium" => Some(TodoPriority::Medium),
                            "low" => Some(TodoPriority::Low),
                            _ => None,
                        }),
                    },
                    "add" => {
                        let content = match params.get("content").and_then(|v| v.as_str()) {
                            Some(c) => c.to_string(),
                            None => {
                                chat_ref.send_message(ChatMessage::ToolResult {
                                    id,
                                    result: "Error: Missing 'content' field for add operation".to_string(),
                                })?;
                                return Ok(());
                            }
                        };
                        let priority = match params.get("priority").and_then(|v| v.as_str()).unwrap_or("medium") {
                            "high" => TodoPriority::High,
                            "low" => TodoPriority::Low,
                            _ => TodoPriority::Medium,
                        };
                        TodoOperation::Add { session_id, content, priority }
                    },
                    "update" => {
                        let todo_id = match params.get("id").and_then(|v| v.as_str()) {
                            Some(id) => id.to_string(),
                            None => {
                                chat_ref.send_message(ChatMessage::ToolResult {
                                    id,
                                    result: "Error: Missing 'id' field for update operation".to_string(),
                                })?;
                                return Ok(());
                            }
                        };
                        TodoOperation::Update {
                            session_id,
                            id: todo_id,
                            content: params.get("content").and_then(|v| v.as_str()).map(|s| s.to_string()),
                            status: params.get("status").and_then(|v| v.as_str()).and_then(|s| match s {
                                "pending" => Some(TodoStatus::Pending),
                                "in_progress" => Some(TodoStatus::InProgress),
                                "completed" => Some(TodoStatus::Completed),
                                _ => None,
                            }),
                            priority: params.get("priority").and_then(|v| v.as_str()).and_then(|s| match s {
                                "high" => Some(TodoPriority::High),
                                "medium" => Some(TodoPriority::Medium),
                                "low" => Some(TodoPriority::Low),
                                _ => None,
                            }),
                        }
                    },
                    "remove" => {
                        let todo_id = match params.get("id").and_then(|v| v.as_str()) {
                            Some(id) => id.to_string(),
                            None => {
                                chat_ref.send_message(ChatMessage::ToolResult {
                                    id,
                                    result: "Error: Missing 'id' field for remove operation".to_string(),
                                })?;
                                return Ok(());
                            }
                        };
                        TodoOperation::Remove { session_id, id: todo_id }
                    },
                    "clear" => TodoOperation::Clear {
                        session_id,
                        status: params.get("status").and_then(|v| v.as_str()).and_then(|s| match s {
                            "pending" => Some(TodoStatus::Pending),
                            "in_progress" => Some(TodoStatus::InProgress),
                            "completed" => Some(TodoStatus::Completed),
                            _ => None,
                        }),
                    },
                    "stats" => TodoOperation::Stats { session_id },
                    _ => {
                        chat_ref.send_message(ChatMessage::ToolResult {
                            id,
                            result: format!("Error: Unknown operation '{}'", operation_str),
                        })?;
                        return Ok(());
                    }
                };
                
                let result = match operation {
                    TodoOperation::List { session_id, status, priority } => {
                        self.handle_list(&session_id, status, priority).await
                    }
                    
                    TodoOperation::Add { session_id, content, priority } => {
                        match self.add_todo(&session_id, &content, priority).await {
                            Ok(todo_id) => format!("Added todo #{}: {}", todo_id, content),
                            Err(e) => format!("Error adding todo: {}", e),
                        }
                    }
                    
                    TodoOperation::Update { session_id, id: todo_id, content, status, priority } => {
                        match self.update_todo(&session_id, &todo_id, content, status, priority).await {
                            Ok(updated) => {
                                if updated {
                                    format!("Updated todo #{}", todo_id)
                                } else {
                                    format!("Todo #{} not found", todo_id)
                                }
                            }
                            Err(e) => format!("Error updating todo: {}", e),
                        }
                    }
                    
                    TodoOperation::Remove { session_id, id: todo_id } => {
                        match self.remove_todo(&session_id, &todo_id).await {
                            Ok(Some(content)) => format!("Removed todo #{}: {}", todo_id, content),
                            Ok(None) => format!("Todo #{} not found", todo_id),
                            Err(e) => format!("Error removing todo: {}", e),
                        }
                    }
                    
                    TodoOperation::Clear { session_id, status } => {
                        match self.clear_todos(&session_id, status).await {
                            Ok(count) => format!("Cleared {} todos", count),
                            Err(e) => format!("Error clearing todos: {}", e),
                        }
                    }
                    
                    TodoOperation::Stats { session_id } => {
                        match self.get_stats(&session_id).await {
                            Ok(stats) => stats,
                            Err(e) => format!("Error getting stats: {}", e),
                        }
                    }
                };
                
                // Send result back to chat
                chat_ref.send_message(ChatMessage::ToolResult {
                    id,
                    result,
                })?;
            }
            
            ToolMessage::Cancel { id } => {
                tracing::debug!("Cancelling todo operation {}", id);
                // Todo operations are synchronous, nothing to cancel
            }
            
            ToolMessage::StreamUpdate { .. } => {
                // Todo doesn't stream updates
            }
        }
        Ok(())
    }
}

impl TodoActor {
    pub async fn new(config: Config) -> Result<Self> {
        // Get database path from config
        let db_path = config.session.database_path.clone()
            .unwrap_or_else(|| {
                dirs::home_dir()
                    .unwrap_or_else(|| PathBuf::from("/tmp"))
                    .join(".assistant")
                    .join("assistant.db")
            });
        
        // Ensure directory exists
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        
        let database = Database::new(&db_path).await?;
        
        Ok(Self { config, database })
    }
    
    async fn handle_list(&self, session_id: &str, status: Option<TodoStatus>, priority: Option<TodoPriority>) -> String {
        match self.list_todos(session_id, status, priority).await {
            Ok(todos) => {
                if todos.is_empty() {
                    return "No todos found matching the criteria".to_string();
                }
                
                let mut output = String::from("**Todo List:**\n\n");
                
                // Group by priority
                let mut current_priority: Option<TodoPriority> = None;
                for todo in todos {
                    if current_priority != Some(todo.priority) {
                        current_priority = Some(todo.priority);
                        output.push_str(&format!("**{} Priority:**\n", 
                            match todo.priority {
                                TodoPriority::High => "High",
                                TodoPriority::Medium => "Medium",
                                TodoPriority::Low => "Low",
                            }
                        ));
                    }
                    
                    let status_icon = match todo.status {
                        TodoStatus::Pending => "○",
                        TodoStatus::InProgress => "◐",
                        TodoStatus::Completed => "●",
                    };
                    
                    output.push_str(&format!("  {} {} - {}\n", status_icon, todo.id, todo.content));
                }
                
                output
            }
            Err(e) => format!("Error listing todos: {}", e),
        }
    }
    
    async fn list_todos(&self, session_id: &str, status: Option<TodoStatus>, priority: Option<TodoPriority>) -> Result<Vec<Todo>> {
        let mut query = String::from(
            "SELECT id, content, status, priority FROM todos WHERE session_id = ?1"
        );
        
        let mut params: Vec<String> = vec![session_id.to_string()];
        
        if let Some(ref s) = status {
            query.push_str(" AND status = ?2");
            params.push(match s {
                TodoStatus::Pending => "pending",
                TodoStatus::InProgress => "in_progress",
                TodoStatus::Completed => "completed",
            }.to_string());
        }
        
        if let Some(ref p) = priority {
            let param_num = if status.is_some() { 3 } else { 2 };
            query.push_str(&format!(" AND priority = ?{}", param_num));
            params.push(match p {
                TodoPriority::High => "high",
                TodoPriority::Medium => "medium",
                TodoPriority::Low => "low",
            }.to_string());
        }
        
        query.push_str(" ORDER BY CASE priority WHEN 'high' THEN 0 WHEN 'medium' THEN 1 ELSE 2 END, CASE status WHEN 'in_progress' THEN 0 WHEN 'pending' THEN 1 ELSE 2 END");
        
        let mut sql_query = sqlx::query(&query);
        for param in params {
            sql_query = sql_query.bind(param);
        }
        
        let rows = sql_query.fetch_all(self.database.pool()).await?;
        
        let todos = rows.into_iter().map(|row| {
            let status_str: String = row.get(2);
            let priority_str: String = row.get(3);
            
            Todo {
                id: row.get(0),
                content: row.get(1),
                status: match status_str.as_str() {
                    "pending" => TodoStatus::Pending,
                    "in_progress" => TodoStatus::InProgress,
                    "completed" => TodoStatus::Completed,
                    _ => TodoStatus::Pending,
                },
                priority: match priority_str.as_str() {
                    "high" => TodoPriority::High,
                    "medium" => TodoPriority::Medium,
                    "low" => TodoPriority::Low,
                    _ => TodoPriority::Medium,
                },
            }
        }).collect();
        
        Ok(todos)
    }
    
    async fn add_todo(&self, session_id: &str, content: &str, priority: TodoPriority) -> Result<String> {
        let id = Uuid::new_v4().to_string();
        let now = Utc::now();
        
        let priority_str = match priority {
            TodoPriority::High => "high",
            TodoPriority::Medium => "medium",
            TodoPriority::Low => "low",
        };
        
        sqlx::query(
            r#"
            INSERT INTO todos (id, session_id, content, status, priority, created_at, updated_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
            "#,
        )
        .bind(&id)
        .bind(session_id)
        .bind(content)
        .bind("pending")
        .bind(priority_str)
        .bind(&now)
        .bind(&now)
        .execute(self.database.pool())
        .await?;
        
        Ok(id)
    }
    
    async fn update_todo(
        &self,
        session_id: &str,
        id: &str,
        content: Option<String>,
        status: Option<TodoStatus>,
        priority: Option<TodoPriority>,
    ) -> Result<bool> {
        let mut updates = Vec::new();
        let mut params: Vec<String> = Vec::new();
        
        if let Some(c) = content {
            updates.push("content = ?");
            params.push(c);
        }
        
        if let Some(s) = status {
            updates.push("status = ?");
            params.push(match s {
                TodoStatus::Pending => "pending",
                TodoStatus::InProgress => "in_progress",
                TodoStatus::Completed => "completed",
            }.to_string());
        }
        
        if let Some(p) = priority {
            updates.push("priority = ?");
            params.push(match p {
                TodoPriority::High => "high",
                TodoPriority::Medium => "medium",
                TodoPriority::Low => "low",
            }.to_string());
        }
        
        if updates.is_empty() {
            return Ok(false);
        }
        
        updates.push("updated_at = ?");
        
        let query = format!(
            "UPDATE todos SET {} WHERE id = ? AND session_id = ?",
            updates.join(", ")
        );
        
        let mut sql_query = sqlx::query(&query);
        for param in params {
            sql_query = sql_query.bind(param);
        }
        sql_query = sql_query.bind(Utc::now());
        sql_query = sql_query.bind(id);
        sql_query = sql_query.bind(session_id);
        
        let result = sql_query.execute(self.database.pool()).await?;
        Ok(result.rows_affected() > 0)
    }
    
    async fn remove_todo(&self, session_id: &str, id: &str) -> Result<Option<String>> {
        // First get the content
        let row = sqlx::query(
            "SELECT content FROM todos WHERE id = ?1 AND session_id = ?2"
        )
        .bind(id)
        .bind(session_id)
        .fetch_optional(self.database.pool())
        .await?;
        
        if let Some(row) = row {
            let content: String = row.get(0);
            
            // Delete the todo
            sqlx::query(
                "DELETE FROM todos WHERE id = ?1 AND session_id = ?2"
            )
            .bind(id)
            .bind(session_id)
            .execute(self.database.pool())
            .await?;
            
            Ok(Some(content))
        } else {
            Ok(None)
        }
    }
    
    async fn clear_todos(&self, session_id: &str, status: Option<TodoStatus>) -> Result<u64> {
        let query = if let Some(s) = status {
            let status_str = match s {
                TodoStatus::Pending => "pending",
                TodoStatus::InProgress => "in_progress",
                TodoStatus::Completed => "completed",
            };
            
            sqlx::query(
                "DELETE FROM todos WHERE session_id = ?1 AND status = ?2"
            )
            .bind(session_id)
            .bind(status_str)
        } else {
            sqlx::query(
                "DELETE FROM todos WHERE session_id = ?1"
            )
            .bind(session_id)
        };
        
        let result = query.execute(self.database.pool()).await?;
        Ok(result.rows_affected())
    }
    
    async fn get_stats(&self, session_id: &str) -> Result<String> {
        let stats = sqlx::query(
            r#"
            SELECT 
                COUNT(*) as total,
                SUM(CASE WHEN status = 'completed' THEN 1 ELSE 0 END) as completed,
                SUM(CASE WHEN status = 'in_progress' THEN 1 ELSE 0 END) as in_progress,
                SUM(CASE WHEN status = 'pending' THEN 1 ELSE 0 END) as pending,
                SUM(CASE WHEN priority = 'high' THEN 1 ELSE 0 END) as high,
                SUM(CASE WHEN priority = 'medium' THEN 1 ELSE 0 END) as medium,
                SUM(CASE WHEN priority = 'low' THEN 1 ELSE 0 END) as low
            FROM todos
            WHERE session_id = ?1
            "#
        )
        .bind(session_id)
        .fetch_one(self.database.pool())
        .await?;
        
        let total: i64 = stats.get(0);
        let completed: i64 = stats.get(1);
        let in_progress: i64 = stats.get(2);
        let pending: i64 = stats.get(3);
        let high: i64 = stats.get(4);
        let medium: i64 = stats.get(5);
        let low: i64 = stats.get(6);
        
        Ok(format!(
            "**Todo Statistics:**\n\n\
            **Total:** {} todos\n\n\
            **By Status:**\n\
            - Completed: {}\n\
            - In Progress: {}\n\
            - Pending: {}\n\n\
            **By Priority:**\n\
            - High: {}\n\
            - Medium: {}\n\
            - Low: {}",
            total, completed, in_progress, pending, high, medium, low
        ))
    }
}