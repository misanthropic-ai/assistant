use assistant_core::actors::tools::todo::TodoActor;
use assistant_core::messages::ToolMessage;
use assistant_core::messages::ChatMessage;
use assistant_core::config::Config;
use ractor::{Actor, ActorRef};
use serde_json::json;
use tokio::sync::mpsc;
use uuid::Uuid;
use tempfile::TempDir;
use assistant_core::persistence::Database;

// Mock ChatActor for testing
struct MockChatActor {
    sender: mpsc::UnboundedSender<ChatMessage>,
}

struct MockChatState;

impl Actor for MockChatActor {
    type Msg = ChatMessage;
    type State = MockChatState;
    type Arguments = mpsc::UnboundedSender<ChatMessage>;
    
    async fn pre_start(
        &self,
        _myself: ActorRef<Self::Msg>,
        _sender: Self::Arguments,
    ) -> Result<Self::State, ractor::ActorProcessingErr> {
        Ok(MockChatState)
    }
    
    async fn handle(
        &self,
        _myself: ActorRef<Self::Msg>,
        msg: Self::Msg,
        _state: &mut Self::State,
    ) -> Result<(), ractor::ActorProcessingErr> {
        // Forward message to test channel
        let _ = self.sender.send(msg);
        Ok(())
    }
}

async fn setup_test() -> (Config, ActorRef<ChatMessage>, mpsc::UnboundedReceiver<ChatMessage>, TempDir) {
    let mut config = Config::default();
    config.api_key = "test-api-key".to_string();
    
    // Set up a temporary database
    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path().join("test.db");
    config.session.database_path = Some(db_path.clone());
    
    let (tx, rx) = mpsc::unbounded_channel();
    let mock_chat = MockChatActor { sender: tx.clone() };
    let (chat_ref, _) = Actor::spawn(None, mock_chat, tx).await.unwrap();
    
    (config, chat_ref, rx, temp_dir)
}

#[tokio::test]
async fn test_todo_write_empty_list() {
    let (config, chat_ref, mut rx, _temp_dir) = setup_test().await;
    
    // Create TodoActor
    let todo_actor = TodoActor::new(config.clone()).await.unwrap();
    let (todo_ref, _) = Actor::spawn(None, todo_actor, config.clone()).await.unwrap();
    
    // Create default session in the database
    let db = Database::new(&config.session.database_path.as_ref().unwrap()).await.unwrap();
    sqlx::query("INSERT OR IGNORE INTO sessions (id) VALUES ('default')")
        .execute(db.pool())
        .await
        .unwrap();
    
    // List todos (empty)
    let id = Uuid::new_v4();
    let params = json!({
        "operation": "list"
    });
    
    todo_ref.send_message(ToolMessage::Execute {
        id,
        params,
        chat_ref,
    }).unwrap();
    
    // Wait for response
    let response = rx.recv().await.unwrap();
    match response {
        ChatMessage::ToolResult { id: res_id, result } => {
            assert_eq!(res_id, id);
            assert!(result.contains("No todos"));
        }
        _ => panic!("Expected ToolResult message"),
    }
}

#[tokio::test]
async fn test_todo_write_single_todo() {
    let (config, chat_ref, mut rx, _temp_dir) = setup_test().await;
    
    // Create TodoActor
    let todo_actor = TodoActor::new(config.clone()).await.unwrap();
    let (todo_ref, _) = Actor::spawn(None, todo_actor, config.clone()).await.unwrap();
    
    // Create default session in the database
    let db = Database::new(&config.session.database_path.as_ref().unwrap()).await.unwrap();
    sqlx::query("INSERT OR IGNORE INTO sessions (id) VALUES ('default')")
        .execute(db.pool())
        .await
        .unwrap();
    
    // Add a single todo
    let add_id = Uuid::new_v4();
    let add_params = json!({
        "operation": "add",
        "content": "Implement feature X",
        "priority": "high"
    });
    
    todo_ref.send_message(ToolMessage::Execute {
        id: add_id,
        params: add_params,
        chat_ref: chat_ref.clone(),
    }).unwrap();
    
    // Wait for add response
    let add_response = rx.recv().await.unwrap();
    match &add_response {
        ChatMessage::ToolResult { result, .. } => {
            println!("Add result: {}", result);
        }
        _ => panic!("Expected ToolResult for add"),
    }
    
    // Now list todos to see the result
    let list_id = Uuid::new_v4();
    let list_params = json!({
        "operation": "list"
    });
    
    todo_ref.send_message(ToolMessage::Execute {
        id: list_id,
        params: list_params,
        chat_ref,
    }).unwrap();
    
    // Wait for list response
    let response = rx.recv().await.unwrap();
    match response {
        ChatMessage::ToolResult { id: res_id, result } => {
            assert_eq!(res_id, list_id);
            println!("List result: {}", result);
            assert!(result.contains("Todo List:") || result.contains("**Todo List:**"));
            assert!(result.contains("High Priority:") || result.contains("**High Priority:**"));
            assert!(result.contains("Implement feature X"));
            assert!(result.contains("○")); // pending icon
        }
        _ => panic!("Expected ToolResult message"),
    }
}

#[tokio::test]
async fn test_todo_write_multiple_priorities() {
    let (config, chat_ref, mut rx, _temp_dir) = setup_test().await;
    
    // Create TodoActor
    let todo_actor = TodoActor::new(config.clone()).await.unwrap();
    let (todo_ref, _) = Actor::spawn(None, todo_actor, config.clone()).await.unwrap();
    
    // Create default session in the database
    let db = Database::new(&config.session.database_path.as_ref().unwrap()).await.unwrap();
    sqlx::query("INSERT OR IGNORE INTO sessions (id) VALUES ('default')")
        .execute(db.pool())
        .await
        .unwrap();
    
    // Add todos with different priorities
    // Add high priority todo
    let add1_params = json!({
        "operation": "add",
        "content": "Critical bug fix",
        "priority": "high"
    });
    todo_ref.send_message(ToolMessage::Execute {
        id: Uuid::new_v4(),
        params: add1_params,
        chat_ref: chat_ref.clone(),
    }).unwrap();
    let _ = rx.recv().await.unwrap();
    
    // Add medium priority todo
    let add2_params = json!({
        "operation": "add",
        "content": "Code review",
        "priority": "medium"
    });
    todo_ref.send_message(ToolMessage::Execute {
        id: Uuid::new_v4(),
        params: add2_params,
        chat_ref: chat_ref.clone(),
    }).unwrap();
    let _ = rx.recv().await.unwrap();
    
    // Add low priority todo
    let add3_params = json!({
        "operation": "add",
        "content": "Update documentation",
        "priority": "low"
    });
    todo_ref.send_message(ToolMessage::Execute {
        id: Uuid::new_v4(),
        params: add3_params,
        chat_ref: chat_ref.clone(),
    }).unwrap();
    let _ = rx.recv().await.unwrap();
    
    // Add another high priority todo
    let add4_params = json!({
        "operation": "add",
        "content": "Deploy to production",
        "priority": "high"
    });
    todo_ref.send_message(ToolMessage::Execute {
        id: Uuid::new_v4(),
        params: add4_params,
        chat_ref: chat_ref.clone(),
    }).unwrap();
    let _ = rx.recv().await.unwrap();
    
    // Now list all todos
    let list_id = Uuid::new_v4();
    let list_params = json!({
        "operation": "list"
    });
    
    todo_ref.send_message(ToolMessage::Execute {
        id: list_id,
        params: list_params,
        chat_ref,
    }).unwrap();
    
    // Wait for list response
    let response = rx.recv().await.unwrap();
    match response {
        ChatMessage::ToolResult { id: res_id, result } => {
            assert_eq!(res_id, list_id);
            assert!(result.contains("High Priority:"));
            assert!(result.contains("Critical bug fix"));
            assert!(result.contains("Deploy to production"));
            assert!(result.contains("Medium Priority:"));
            assert!(result.contains("Code review"));
            assert!(result.contains("Low Priority:"));
            assert!(result.contains("Update documentation"));
        }
        _ => panic!("Expected ToolResult message"),
    }
}

#[tokio::test]
async fn test_todo_write_status_ordering() {
    let (config, chat_ref, mut rx, _temp_dir) = setup_test().await;
    
    // Create TodoActor
    let todo_actor = TodoActor::new(config.clone()).await.unwrap();
    let (todo_ref, _) = Actor::spawn(None, todo_actor, config.clone()).await.unwrap();
    
    // Create default session in the database
    let db = Database::new(&config.session.database_path.as_ref().unwrap()).await.unwrap();
    sqlx::query("INSERT OR IGNORE INTO sessions (id) VALUES ('default')")
        .execute(db.pool())
        .await
        .unwrap();
    
    // Add todos with same priority but different statuses
    // Add pending task
    let add1_params = json!({
        "operation": "add",
        "content": "Pending task",
        "priority": "high"
    });
    todo_ref.send_message(ToolMessage::Execute {
        id: Uuid::new_v4(),
        params: add1_params,
        chat_ref: chat_ref.clone(),
    }).unwrap();
    let _ = rx.recv().await.unwrap();
    
    // Add another pending task
    let add2_params = json!({
        "operation": "add",
        "content": "In progress task",
        "priority": "high"
    });
    todo_ref.send_message(ToolMessage::Execute {
        id: Uuid::new_v4(),
        params: add2_params,
        chat_ref: chat_ref.clone(),
    }).unwrap();
    let add2_response = rx.recv().await.unwrap();
    
    // Extract ID from response to update it
    let in_progress_id = match add2_response {
        ChatMessage::ToolResult { result, .. } => {
            // Extract ID from "Added todo #ID: content"
            if let Some(id_part) = result.split('#').nth(1) {
                if let Some(id) = id_part.split(':').nth(0) {
                    id.trim().to_string()
                } else {
                    panic!("Could not extract ID from: {}", result);
                }
            } else {
                panic!("Could not extract ID from: {}", result);
            }
        }
        _ => panic!("Expected ToolResult"),
    };
    
    // Update second task to in_progress
    let update_params = json!({
        "operation": "update",
        "id": in_progress_id,
        "status": "in_progress"
    });
    todo_ref.send_message(ToolMessage::Execute {
        id: Uuid::new_v4(),
        params: update_params,
        chat_ref: chat_ref.clone(),
    }).unwrap();
    let _ = rx.recv().await.unwrap();
    
    // Add completed task
    let add3_params = json!({
        "operation": "add",
        "content": "Completed task",
        "priority": "high"
    });
    todo_ref.send_message(ToolMessage::Execute {
        id: Uuid::new_v4(),
        params: add3_params,
        chat_ref: chat_ref.clone(),
    }).unwrap();
    let add3_response = rx.recv().await.unwrap();
    
    // Extract ID and update to completed
    let completed_id = match add3_response {
        ChatMessage::ToolResult { result, .. } => {
            if let Some(id_part) = result.split('#').nth(1) {
                if let Some(id) = id_part.split(':').nth(0) {
                    id.trim().to_string()
                } else {
                    panic!("Could not extract ID from: {}", result);
                }
            } else {
                panic!("Could not extract ID from: {}", result);
            }
        }
        _ => panic!("Expected ToolResult"),
    };
    
    let update2_params = json!({
        "operation": "update",
        "id": completed_id,
        "status": "completed"
    });
    todo_ref.send_message(ToolMessage::Execute {
        id: Uuid::new_v4(),
        params: update2_params,
        chat_ref: chat_ref.clone(),
    }).unwrap();
    let _ = rx.recv().await.unwrap();
    
    // Now list all todos
    let list_id = Uuid::new_v4();
    let list_params = json!({
        "operation": "list"
    });
    
    todo_ref.send_message(ToolMessage::Execute {
        id: list_id,
        params: list_params,
        chat_ref,
    }).unwrap();
    
    // Wait for response
    let response = rx.recv().await.unwrap();
    match response {
        ChatMessage::ToolResult { id: res_id, result } => {
            assert_eq!(res_id, list_id);
            // Verify status ordering (in_progress first, then pending, then completed)
            let lines: Vec<&str> = result.lines().collect();
            let mut found_order = Vec::new();
            for line in lines {
                if line.contains("In progress task") {
                    found_order.push("in_progress");
                } else if line.contains("Pending task") {
                    found_order.push("pending");
                } else if line.contains("Completed task") {
                    found_order.push("completed");
                }
            }
            assert_eq!(found_order, vec!["in_progress", "pending", "completed"]);
        }
        _ => panic!("Expected ToolResult message"),
    }
}

#[tokio::test]
async fn test_todo_write_update_existing() {
    let (config, chat_ref, mut rx, _temp_dir) = setup_test().await;
    
    // Create TodoActor
    let todo_actor = TodoActor::new(config.clone()).await.unwrap();
    let (todo_ref, _) = Actor::spawn(None, todo_actor, config.clone()).await.unwrap();
    
    // Create default session in the database
    let db = Database::new(&config.session.database_path.as_ref().unwrap()).await.unwrap();
    sqlx::query("INSERT OR IGNORE INTO sessions (id) VALUES ('default')")
        .execute(db.pool())
        .await
        .unwrap();
    
    // Add initial todos
    let add1_params = json!({
        "operation": "add",
        "content": "Task 1",
        "priority": "high"
    });
    todo_ref.send_message(ToolMessage::Execute {
        id: Uuid::new_v4(),
        params: add1_params,
        chat_ref: chat_ref.clone(),
    }).unwrap();
    let task1_response = rx.recv().await.unwrap();
    
    let task1_id = match task1_response {
        ChatMessage::ToolResult { result, .. } => {
            if let Some(id_part) = result.split('#').nth(1) {
                if let Some(id) = id_part.split(':').nth(0) {
                    id.trim().to_string()
                } else {
                    panic!("Could not extract ID from: {}", result);
                }
            } else {
                panic!("Could not extract ID from: {}", result);
            }
        }
        _ => panic!("Expected ToolResult"),
    };
    
    let add2_params = json!({
        "operation": "add",
        "content": "Task 2",
        "priority": "medium"
    });
    todo_ref.send_message(ToolMessage::Execute {
        id: Uuid::new_v4(),
        params: add2_params,
        chat_ref: chat_ref.clone(),
    }).unwrap();
    let task2_response = rx.recv().await.unwrap();
    
    let task2_id = match task2_response {
        ChatMessage::ToolResult { result, .. } => {
            if let Some(id_part) = result.split('#').nth(1) {
                if let Some(id) = id_part.split(':').nth(0) {
                    id.trim().to_string()
                } else {
                    panic!("Could not extract ID from: {}", result);
                }
            } else {
                panic!("Could not extract ID from: {}", result);
            }
        }
        _ => panic!("Expected ToolResult"),
    };
    
    // Update task 1 to completed
    let update1_params = json!({
        "operation": "update",
        "id": task1_id,
        "status": "completed"
    });
    todo_ref.send_message(ToolMessage::Execute {
        id: Uuid::new_v4(),
        params: update1_params,
        chat_ref: chat_ref.clone(),
    }).unwrap();
    let _ = rx.recv().await.unwrap();
    
    // Update task 2 to in_progress
    let update2_params = json!({
        "operation": "update",
        "id": task2_id,
        "status": "in_progress"
    });
    todo_ref.send_message(ToolMessage::Execute {
        id: Uuid::new_v4(),
        params: update2_params,
        chat_ref: chat_ref.clone(),
    }).unwrap();
    let _ = rx.recv().await.unwrap();
    
    // Add task 3
    let add3_params = json!({
        "operation": "add",
        "content": "Task 3",
        "priority": "low"
    });
    todo_ref.send_message(ToolMessage::Execute {
        id: Uuid::new_v4(),
        params: add3_params,
        chat_ref: chat_ref.clone(),
    }).unwrap();
    let _ = rx.recv().await.unwrap();
    
    // Get stats
    let stats_id = Uuid::new_v4();
    let stats_params = json!({
        "operation": "stats"
    });
    
    todo_ref.send_message(ToolMessage::Execute {
        id: stats_id,
        params: stats_params,
        chat_ref,
    }).unwrap();
    
    // Wait for response
    let response = rx.recv().await.unwrap();
    match response {
        ChatMessage::ToolResult { id: res_id, result } => {
            assert_eq!(res_id, stats_id);
            assert!(result.contains("Total:") && result.contains("3 todos"));
            assert!(result.contains("Completed: 1"));
            assert!(result.contains("In Progress: 1"));
            assert!(result.contains("Pending: 1"));
        }
        _ => panic!("Expected ToolResult message"),
    }
}

#[tokio::test]
async fn test_todo_write_invalid_parameters() {
    let (config, chat_ref, mut rx, _temp_dir) = setup_test().await;
    
    // Create TodoActor
    let todo_actor = TodoActor::new(config.clone()).await.unwrap();
    let (todo_ref, _) = Actor::spawn(None, todo_actor, config.clone()).await.unwrap();
    
    // Create default session in the database
    let db = Database::new(&config.session.database_path.as_ref().unwrap()).await.unwrap();
    sqlx::query("INSERT OR IGNORE INTO sessions (id) VALUES ('default')")
        .execute(db.pool())
        .await
        .unwrap();
    
    // Send invalid parameters
    let id = Uuid::new_v4();
    let params = json!({
        "invalid": "parameters"
    });
    
    todo_ref.send_message(ToolMessage::Execute {
        id,
        params,
        chat_ref,
    }).unwrap();
    
    // Wait for response
    let response = rx.recv().await.unwrap();
    match response {
        ChatMessage::ToolResult { id: res_id, result } => {
            assert_eq!(res_id, id);
            assert!(result.contains("Error"));
            assert!(result.contains("Missing 'operation' field"));
        }
        _ => panic!("Expected ToolResult message"),
    }
}

#[tokio::test]
async fn test_todo_write_invalid_status() {
    let (config, chat_ref, mut rx, _temp_dir) = setup_test().await;
    
    // Create TodoActor
    let todo_actor = TodoActor::new(config.clone()).await.unwrap();
    let (todo_ref, _) = Actor::spawn(None, todo_actor, config.clone()).await.unwrap();
    
    // Create default session in the database
    let db = Database::new(&config.session.database_path.as_ref().unwrap()).await.unwrap();
    sqlx::query("INSERT OR IGNORE INTO sessions (id) VALUES ('default')")
        .execute(db.pool())
        .await
        .unwrap();
    
    // First add a todo
    let add_params = json!({
        "operation": "add",
        "content": "Test task",
        "priority": "high"
    });
    todo_ref.send_message(ToolMessage::Execute {
        id: Uuid::new_v4(),
        params: add_params,
        chat_ref: chat_ref.clone(),
    }).unwrap();
    let add_response = rx.recv().await.unwrap();
    
    let todo_id = match add_response {
        ChatMessage::ToolResult { result, .. } => {
            if let Some(id_part) = result.split('#').nth(1) {
                if let Some(id) = id_part.split(':').nth(0) {
                    id.trim().to_string()
                } else {
                    panic!("Could not extract ID from: {}", result);
                }
            } else {
                panic!("Could not extract ID from: {}", result);
            }
        }
        _ => panic!("Expected ToolResult"),
    };
    
    // Try to update with invalid status (should be ignored)
    let id = Uuid::new_v4();
    let params = json!({
        "operation": "update",
        "id": todo_id,
        "status": "invalid_status"
    });
    
    todo_ref.send_message(ToolMessage::Execute {
        id,
        params,
        chat_ref,
    }).unwrap();
    
    // Wait for response
    let response = rx.recv().await.unwrap();
    match response {
        ChatMessage::ToolResult { id: res_id, result } => {
            assert_eq!(res_id, id);
            println!("Update result with invalid status: {}", result);
            // The update should succeed but ignore invalid status
            assert!(result.contains("Updated todo") || result.contains("not found"));
        }
        _ => panic!("Expected ToolResult message"),
    }
}

#[tokio::test]
async fn test_todo_write_missing_fields() {
    let (config, chat_ref, mut rx, _temp_dir) = setup_test().await;
    
    // Create TodoActor
    let todo_actor = TodoActor::new(config.clone()).await.unwrap();
    let (todo_ref, _) = Actor::spawn(None, todo_actor, config.clone()).await.unwrap();
    
    // Create default session in the database
    let db = Database::new(&config.session.database_path.as_ref().unwrap()).await.unwrap();
    sqlx::query("INSERT OR IGNORE INTO sessions (id) VALUES ('default')")
        .execute(db.pool())
        .await
        .unwrap();
    
    // Try to add todo without content
    let id = Uuid::new_v4();
    let params = json!({
        "operation": "add",
        "priority": "high"
    });
    
    todo_ref.send_message(ToolMessage::Execute {
        id,
        params,
        chat_ref,
    }).unwrap();
    
    // Wait for response
    let response = rx.recv().await.unwrap();
    match response {
        ChatMessage::ToolResult { id: res_id, result } => {
            assert_eq!(res_id, id);
            assert!(result.contains("Error"));
            assert!(result.contains("Missing 'content' field"));
        }
        _ => panic!("Expected ToolResult message"),
    }
}

#[tokio::test]
async fn test_todo_write_status_icons() {
    let (config, chat_ref, mut rx, _temp_dir) = setup_test().await;
    
    // Create TodoActor
    let todo_actor = TodoActor::new(config.clone()).await.unwrap();
    let (todo_ref, _) = Actor::spawn(None, todo_actor, config.clone()).await.unwrap();
    
    // Create default session in the database
    let db = Database::new(&config.session.database_path.as_ref().unwrap()).await.unwrap();
    sqlx::query("INSERT OR IGNORE INTO sessions (id) VALUES ('default')")
        .execute(db.pool())
        .await
        .unwrap();
    
    // Add pending todo
    let add1_params = json!({
        "operation": "add",
        "content": "Pending",
        "priority": "high"
    });
    todo_ref.send_message(ToolMessage::Execute {
        id: Uuid::new_v4(),
        params: add1_params,
        chat_ref: chat_ref.clone(),
    }).unwrap();
    let _ = rx.recv().await.unwrap();
    
    // Add in progress todo
    let add2_params = json!({
        "operation": "add",
        "content": "In Progress",
        "priority": "high"
    });
    todo_ref.send_message(ToolMessage::Execute {
        id: Uuid::new_v4(),
        params: add2_params,
        chat_ref: chat_ref.clone(),
    }).unwrap();
    let response2 = rx.recv().await.unwrap();
    
    let id2 = match response2 {
        ChatMessage::ToolResult { result, .. } => {
            if let Some(id_part) = result.split('#').nth(1) {
                if let Some(id) = id_part.split(':').nth(0) {
                    id.trim().to_string()
                } else {
                    panic!("Could not extract ID from: {}", result);
                }
            } else {
                panic!("Could not extract ID from: {}", result);
            }
        }
        _ => panic!("Expected ToolResult"),
    };
    
    // Update to in_progress
    let update2_params = json!({
        "operation": "update",
        "id": id2,
        "status": "in_progress"
    });
    todo_ref.send_message(ToolMessage::Execute {
        id: Uuid::new_v4(),
        params: update2_params,
        chat_ref: chat_ref.clone(),
    }).unwrap();
    let _ = rx.recv().await.unwrap();
    
    // Add completed todo
    let add3_params = json!({
        "operation": "add",
        "content": "Completed",
        "priority": "high"
    });
    todo_ref.send_message(ToolMessage::Execute {
        id: Uuid::new_v4(),
        params: add3_params,
        chat_ref: chat_ref.clone(),
    }).unwrap();
    let response3 = rx.recv().await.unwrap();
    
    let id3 = match response3 {
        ChatMessage::ToolResult { result, .. } => {
            if let Some(id_part) = result.split('#').nth(1) {
                if let Some(id) = id_part.split(':').nth(0) {
                    id.trim().to_string()
                } else {
                    panic!("Could not extract ID from: {}", result);
                }
            } else {
                panic!("Could not extract ID from: {}", result);
            }
        }
        _ => panic!("Expected ToolResult"),
    };
    
    // Update to completed
    let update3_params = json!({
        "operation": "update",
        "id": id3,
        "status": "completed"
    });
    todo_ref.send_message(ToolMessage::Execute {
        id: Uuid::new_v4(),
        params: update3_params,
        chat_ref: chat_ref.clone(),
    }).unwrap();
    let _ = rx.recv().await.unwrap();
    
    // List all todos
    let list_id = Uuid::new_v4();
    let list_params = json!({
        "operation": "list"
    });
    
    todo_ref.send_message(ToolMessage::Execute {
        id: list_id,
        params: list_params,
        chat_ref,
    }).unwrap();
    
    // Wait for response
    let response = rx.recv().await.unwrap();
    match response {
        ChatMessage::ToolResult { id: res_id, result } => {
            assert_eq!(res_id, list_id);
            // Check for status icons
            assert!(result.contains("○")); // pending
            assert!(result.contains("◐")); // in_progress
            assert!(result.contains("●")); // completed
        }
        _ => panic!("Expected ToolResult message"),
    }
}