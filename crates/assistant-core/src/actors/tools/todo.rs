use ractor::{Actor, ActorRef, ActorProcessingErr};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use crate::config::Config;
use crate::messages::{ToolMessage, ChatMessage};
use anyhow::Result;

pub struct TodoActor {
    config: Config,
    storage_path: PathBuf,
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
#[serde(tag = "operation", rename_all = "snake_case")]
pub enum TodoOperation {
    List { 
        #[serde(skip_serializing_if = "Option::is_none")]
        status: Option<TodoStatus>,
        #[serde(skip_serializing_if = "Option::is_none")]
        priority: Option<TodoPriority> 
    },
    Add { 
        content: String,
        priority: TodoPriority 
    },
    Update { 
        id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        content: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        status: Option<TodoStatus>,
        #[serde(skip_serializing_if = "Option::is_none")]
        priority: Option<TodoPriority> 
    },
    Remove { 
        id: String 
    },
    Clear { 
        #[serde(skip_serializing_if = "Option::is_none")]
        status: Option<TodoStatus> 
    },
    Stats,
}

pub struct TodoState {
    todos: HashMap<String, Todo>,
    next_id: u32,
}

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
        
        // Load existing todos from disk
        let todos = match self.load_todos() {
            Ok(todos) => {
                tracing::info!("Loaded {} todos from disk", todos.len());
                todos
            }
            Err(e) => {
                tracing::warn!("Failed to load todos from disk: {}", e);
                HashMap::new()
            }
        };
        
        // Find the highest ID to determine next_id
        let next_id = todos.values()
            .filter_map(|todo| todo.id.parse::<u32>().ok())
            .max()
            .unwrap_or(0) + 1;
        
        Ok(TodoState { todos, next_id })
    }
    
    async fn handle(
        &self,
        _myself: ActorRef<Self::Msg>,
        msg: Self::Msg,
        state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        match msg {
            ToolMessage::Execute { id, params, chat_ref } => {
                tracing::info!("Todo tool execution with params: {:?}", params);
                
                // Parse operation
                let operation: TodoOperation = match serde_json::from_value(params) {
                    Ok(op) => op,
                    Err(e) => {
                        chat_ref.send_message(ChatMessage::ToolResult {
                            id,
                            result: format!("Error: Invalid parameters - {}", e),
                        })?;
                        return Ok(());
                    }
                };
                
                let result = match operation {
                    TodoOperation::List { status, priority } => {
                        self.handle_list(state, status, priority)
                    }
                    
                    TodoOperation::Add { content, priority } => {
                        let todo_id = state.next_id.to_string();
                        state.next_id += 1;
                        
                        let todo = Todo {
                            id: todo_id.clone(),
                            content: content.clone(),
                            status: TodoStatus::Pending,
                            priority,
                        };
                        
                        state.todos.insert(todo_id.clone(), todo);
                        self.save_todos(&state.todos)?;
                        
                        format!("Added todo #{}: {}", todo_id, content)
                    }
                    
                    TodoOperation::Update { id: todo_id, content, status, priority } => {
                        match state.todos.get_mut(&todo_id) {
                            Some(todo) => {
                                if let Some(new_content) = content {
                                    todo.content = new_content;
                                }
                                if let Some(new_status) = status {
                                    todo.status = new_status;
                                }
                                if let Some(new_priority) = priority {
                                    todo.priority = new_priority;
                                }
                                
                                self.save_todos(&state.todos)?;
                                format!("Updated todo #{}", todo_id)
                            }
                            None => format!("Todo #{} not found", todo_id),
                        }
                    }
                    
                    TodoOperation::Remove { id: todo_id } => {
                        match state.todos.remove(&todo_id) {
                            Some(todo) => {
                                self.save_todos(&state.todos)?;
                                format!("Removed todo #{}: {}", todo_id, todo.content)
                            }
                            None => format!("Todo #{} not found", todo_id),
                        }
                    }
                    
                    TodoOperation::Clear { status } => {
                        let count = if let Some(filter_status) = status {
                            let to_remove: Vec<String> = state.todos
                                .iter()
                                .filter(|(_, todo)| todo.status == filter_status)
                                .map(|(id, _)| id.clone())
                                .collect();
                            
                            let count = to_remove.len();
                            for id in to_remove {
                                state.todos.remove(&id);
                            }
                            count
                        } else {
                            let count = state.todos.len();
                            state.todos.clear();
                            count
                        };
                        
                        self.save_todos(&state.todos)?;
                        format!("Cleared {} todos", count)
                    }
                    
                    TodoOperation::Stats => {
                        let total = state.todos.len();
                        let completed = state.todos.values().filter(|t| t.status == TodoStatus::Completed).count();
                        let in_progress = state.todos.values().filter(|t| t.status == TodoStatus::InProgress).count();
                        let pending = state.todos.values().filter(|t| t.status == TodoStatus::Pending).count();
                        
                        let high = state.todos.values().filter(|t| t.priority == TodoPriority::High).count();
                        let medium = state.todos.values().filter(|t| t.priority == TodoPriority::Medium).count();
                        let low = state.todos.values().filter(|t| t.priority == TodoPriority::Low).count();
                        
                        format!(
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
                        )
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
    pub fn new(config: Config) -> Self {
        let home_dir = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
        let storage_path = PathBuf::from(home_dir)
            .join(".assistant")
            .join("todos.json");
        
        Self { config, storage_path }
    }
    
    fn handle_list(&self, state: &TodoState, status: Option<TodoStatus>, priority: Option<TodoPriority>) -> String {
        let mut filtered_todos: Vec<&Todo> = state.todos.values()
            .filter(|todo| {
                status.as_ref().map_or(true, |s| &todo.status == s) &&
                priority.as_ref().map_or(true, |p| &todo.priority == p)
            })
            .collect();
        
        if filtered_todos.is_empty() {
            return "No todos found matching the criteria".to_string();
        }
        
        // Sort by priority first, then by status
        filtered_todos.sort_by_key(|t| (
            match t.priority {
                TodoPriority::High => 0,
                TodoPriority::Medium => 1,
                TodoPriority::Low => 2,
            },
            match t.status {
                TodoStatus::InProgress => 0,
                TodoStatus::Pending => 1,
                TodoStatus::Completed => 2,
            }
        ));
        
        let mut output = String::from("**Todo List:**\n\n");
        
        // Group by priority
        let mut current_priority = None;
        for todo in filtered_todos {
            if current_priority != Some(&todo.priority) {
                current_priority = Some(&todo.priority);
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
    
    fn load_todos(&self) -> Result<HashMap<String, Todo>> {
        if !self.storage_path.exists() {
            return Ok(HashMap::new());
        }
        
        let content = std::fs::read_to_string(&self.storage_path)?;
        let todos: HashMap<String, Todo> = serde_json::from_str(&content)?;
        Ok(todos)
    }
    
    fn save_todos(&self, todos: &HashMap<String, Todo>) -> Result<()> {
        // Create directory if it doesn't exist
        if let Some(parent) = self.storage_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        
        let content = serde_json::to_string_pretty(todos)?;
        std::fs::write(&self.storage_path, content)?;
        Ok(())
    }
}