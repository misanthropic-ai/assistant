use ractor::{Actor, ActorRef, ActorProcessingErr};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use crate::config::Config;
use crate::messages::{ToolMessage, ChatMessage};
use uuid::Uuid;

pub struct TodoWriteActor {
    config: Config,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Todo {
    pub id: String,
    pub content: String,
    pub status: TodoStatus,
    pub priority: TodoPriority,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum TodoStatus {
    Pending,
    InProgress,
    Completed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum TodoPriority {
    High,
    Medium,
    Low,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TodoList {
    todos: Vec<Todo>,
}

pub struct TodoWriteState {
    todos: HashMap<String, Todo>,
}

impl Actor for TodoWriteActor {
    type Msg = ToolMessage;
    type State = TodoWriteState;
    type Arguments = Config;
    
    async fn pre_start(
        &self,
        _myself: ActorRef<Self::Msg>,
        _config: Self::Arguments,
    ) -> Result<Self::State, ActorProcessingErr> {
        tracing::debug!("TodoWrite actor starting");
        Ok(TodoWriteState {
            todos: HashMap::new(),
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
                tracing::info!("TodoWrite tool execution with params: {:?}", params);
                
                // Parse the todo list
                let todo_list: TodoList = match serde_json::from_value(params) {
                    Ok(list) => list,
                    Err(e) => {
                        chat_ref.send_message(ChatMessage::ToolResult {
                            id,
                            result: format!("Error: Invalid parameters - {}", e),
                        })?;
                        return Ok(());
                    }
                };
                
                // Update the todo list
                state.todos.clear();
                for todo in todo_list.todos {
                    state.todos.insert(todo.id.clone(), todo);
                }
                
                // Generate a summary of the todo list
                let mut summary = String::from("Todo list updated:\n\n");
                
                let mut high_priority: Vec<&Todo> = Vec::new();
                let mut medium_priority: Vec<&Todo> = Vec::new();
                let mut low_priority: Vec<&Todo> = Vec::new();
                
                for todo in state.todos.values() {
                    match todo.priority {
                        TodoPriority::High => high_priority.push(todo),
                        TodoPriority::Medium => medium_priority.push(todo),
                        TodoPriority::Low => low_priority.push(todo),
                    }
                }
                
                // Sort by status within each priority group
                let sort_todos = |todos: &mut Vec<&Todo>| {
                    todos.sort_by_key(|t| match t.status {
                        TodoStatus::InProgress => 0,
                        TodoStatus::Pending => 1,
                        TodoStatus::Completed => 2,
                    });
                };
                
                sort_todos(&mut high_priority);
                sort_todos(&mut medium_priority);
                sort_todos(&mut low_priority);
                
                // Format the summary
                let format_todos = |todos: &[&Todo], priority: &str| -> String {
                    if todos.is_empty() {
                        return String::new();
                    }
                    let mut output = format!("**{} Priority:**\n", priority);
                    for todo in todos {
                        let status_icon = match todo.status {
                            TodoStatus::Pending => "○",
                            TodoStatus::InProgress => "◐",
                            TodoStatus::Completed => "●",
                        };
                        output.push_str(&format!("  {} {} - {}\n", status_icon, todo.id, todo.content));
                    }
                    output.push('\n');
                    output
                };
                
                summary.push_str(&format_todos(&high_priority, "High"));
                summary.push_str(&format_todos(&medium_priority, "Medium"));
                summary.push_str(&format_todos(&low_priority, "Low"));
                
                // Add statistics
                let total = state.todos.len();
                let completed = state.todos.values().filter(|t| t.status == TodoStatus::Completed).count();
                let in_progress = state.todos.values().filter(|t| t.status == TodoStatus::InProgress).count();
                let pending = state.todos.values().filter(|t| t.status == TodoStatus::Pending).count();
                
                summary.push_str(&format!(
                    "**Summary:** {} total ({} completed, {} in progress, {} pending)",
                    total, completed, in_progress, pending
                ));
                
                // Send result back to chat
                chat_ref.send_message(ChatMessage::ToolResult {
                    id,
                    result: summary,
                })?;
            }
            
            ToolMessage::Cancel { id } => {
                tracing::debug!("Cancelling todo write operation {}", id);
                // Todo operations are synchronous, nothing to cancel
            }
            
            ToolMessage::StreamUpdate { .. } => {
                // TodoWrite doesn't stream updates
            }
        }
        Ok(())
    }
}

impl TodoWriteActor {
    pub fn new(config: Config) -> Self {
        Self { config }
    }
}