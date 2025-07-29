use assistant_core::actors::tools::{TodoWriteActor, ToolMessage};
use assistant_core::messages::ChatMessage;
use assistant_core::config::Config;
use ractor::{Actor, ActorRef};
use serde_json::json;
use tokio::sync::mpsc;
use uuid::Uuid;

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

async fn setup_test() -> (Config, ActorRef<ChatMessage>, mpsc::UnboundedReceiver<ChatMessage>) {
    let config = Config::default();
    
    let (tx, rx) = mpsc::unbounded_channel();
    let mock_chat = MockChatActor { sender: tx.clone() };
    let (chat_ref, _) = Actor::spawn(None, mock_chat, tx).await.unwrap();
    
    (config, chat_ref, rx)
}

#[tokio::test]
async fn test_todo_write_empty_list() {
    let (config, chat_ref, mut rx) = setup_test().await;
    
    // Create TodoWriteActor
    let todo_write_actor = TodoWriteActor::new(config.clone());
    let (todo_write_ref, _) = Actor::spawn(None, todo_write_actor, config).await.unwrap();
    
    // Send empty todo list
    let id = Uuid::new_v4();
    let params = json!({
        "todos": []
    });
    
    todo_write_ref.send_message(ToolMessage::Execute {
        id,
        params,
        chat_ref,
    }).unwrap();
    
    // Wait for response
    let response = rx.recv().await.unwrap();
    match response {
        ChatMessage::ToolResult { id: res_id, result } => {
            assert_eq!(res_id, id);
            assert!(result.contains("Todo list updated"));
            assert!(result.contains("0 total"));
        }
        _ => panic!("Expected ToolResult message"),
    }
}

#[tokio::test]
async fn test_todo_write_single_todo() {
    let (config, chat_ref, mut rx) = setup_test().await;
    
    // Create TodoWriteActor
    let todo_write_actor = TodoWriteActor::new(config.clone());
    let (todo_write_ref, _) = Actor::spawn(None, todo_write_actor, config).await.unwrap();
    
    // Send single todo
    let id = Uuid::new_v4();
    let params = json!({
        "todos": [
            {
                "id": "1",
                "content": "Implement feature X",
                "status": "pending",
                "priority": "high"
            }
        ]
    });
    
    todo_write_ref.send_message(ToolMessage::Execute {
        id,
        params,
        chat_ref,
    }).unwrap();
    
    // Wait for response
    let response = rx.recv().await.unwrap();
    match response {
        ChatMessage::ToolResult { id: res_id, result } => {
            assert_eq!(res_id, id);
            assert!(result.contains("Todo list updated"));
            assert!(result.contains("High Priority"));
            assert!(result.contains("Implement feature X"));
            assert!(result.contains("1 total"));
            assert!(result.contains("1 pending"));
        }
        _ => panic!("Expected ToolResult message"),
    }
}

#[tokio::test]
async fn test_todo_write_multiple_priorities() {
    let (config, chat_ref, mut rx) = setup_test().await;
    
    // Create TodoWriteActor
    let todo_write_actor = TodoWriteActor::new(config.clone());
    let (todo_write_ref, _) = Actor::spawn(None, todo_write_actor, config).await.unwrap();
    
    // Send todos with different priorities
    let id = Uuid::new_v4();
    let params = json!({
        "todos": [
            {
                "id": "1",
                "content": "Critical bug fix",
                "status": "in_progress",
                "priority": "high"
            },
            {
                "id": "2",
                "content": "Code review",
                "status": "pending",
                "priority": "medium"
            },
            {
                "id": "3",
                "content": "Update documentation",
                "status": "pending",
                "priority": "low"
            },
            {
                "id": "4",
                "content": "Deploy to production",
                "status": "pending",
                "priority": "high"
            }
        ]
    });
    
    todo_write_ref.send_message(ToolMessage::Execute {
        id,
        params,
        chat_ref,
    }).unwrap();
    
    // Wait for response
    let response = rx.recv().await.unwrap();
    match response {
        ChatMessage::ToolResult { id: res_id, result } => {
            assert_eq!(res_id, id);
            assert!(result.contains("High Priority"));
            assert!(result.contains("Critical bug fix"));
            assert!(result.contains("Deploy to production"));
            assert!(result.contains("Medium Priority"));
            assert!(result.contains("Code review"));
            assert!(result.contains("Low Priority"));
            assert!(result.contains("Update documentation"));
            assert!(result.contains("4 total"));
            assert!(result.contains("1 in progress"));
            assert!(result.contains("3 pending"));
        }
        _ => panic!("Expected ToolResult message"),
    }
}

#[tokio::test]
async fn test_todo_write_status_ordering() {
    let (config, chat_ref, mut rx) = setup_test().await;
    
    // Create TodoWriteActor
    let todo_write_actor = TodoWriteActor::new(config.clone());
    let (todo_write_ref, _) = Actor::spawn(None, todo_write_actor, config).await.unwrap();
    
    // Send todos with same priority but different statuses
    let id = Uuid::new_v4();
    let params = json!({
        "todos": [
            {
                "id": "1",
                "content": "Completed task",
                "status": "completed",
                "priority": "high"
            },
            {
                "id": "2",
                "content": "In progress task",
                "status": "in_progress",
                "priority": "high"
            },
            {
                "id": "3",
                "content": "Pending task",
                "status": "pending",
                "priority": "high"
            }
        ]
    });
    
    todo_write_ref.send_message(ToolMessage::Execute {
        id,
        params,
        chat_ref,
    }).unwrap();
    
    // Wait for response
    let response = rx.recv().await.unwrap();
    match response {
        ChatMessage::ToolResult { id: res_id, result } => {
            assert_eq!(res_id, id);
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
    let (config, chat_ref, mut rx) = setup_test().await;
    
    // Create TodoWriteActor
    let todo_write_actor = TodoWriteActor::new(config.clone());
    let (todo_write_ref, _) = Actor::spawn(None, todo_write_actor, config).await.unwrap();
    
    // Send initial todos
    let params1 = json!({
        "todos": [
            {
                "id": "1",
                "content": "Task 1",
                "status": "pending",
                "priority": "high"
            },
            {
                "id": "2",
                "content": "Task 2",
                "status": "pending",
                "priority": "medium"
            }
        ]
    });
    
    todo_write_ref.send_message(ToolMessage::Execute {
        id: Uuid::new_v4(),
        params: params1,
        chat_ref: chat_ref.clone(),
    }).unwrap();
    
    // Consume first response
    let _ = rx.recv().await.unwrap();
    
    // Update the todos
    let id = Uuid::new_v4();
    let params2 = json!({
        "todos": [
            {
                "id": "1",
                "content": "Task 1",
                "status": "completed",
                "priority": "high"
            },
            {
                "id": "2",
                "content": "Task 2",
                "status": "in_progress",
                "priority": "medium"
            },
            {
                "id": "3",
                "content": "Task 3",
                "status": "pending",
                "priority": "low"
            }
        ]
    });
    
    todo_write_ref.send_message(ToolMessage::Execute {
        id,
        params: params2,
        chat_ref,
    }).unwrap();
    
    // Wait for response
    let response = rx.recv().await.unwrap();
    match response {
        ChatMessage::ToolResult { id: res_id, result } => {
            assert_eq!(res_id, id);
            assert!(result.contains("3 total"));
            assert!(result.contains("1 completed"));
            assert!(result.contains("1 in progress"));
            assert!(result.contains("1 pending"));
        }
        _ => panic!("Expected ToolResult message"),
    }
}

#[tokio::test]
async fn test_todo_write_invalid_parameters() {
    let (config, chat_ref, mut rx) = setup_test().await;
    
    // Create TodoWriteActor
    let todo_write_actor = TodoWriteActor::new(config.clone());
    let (todo_write_ref, _) = Actor::spawn(None, todo_write_actor, config).await.unwrap();
    
    // Send invalid parameters
    let id = Uuid::new_v4();
    let params = json!({
        "invalid": "parameters"
    });
    
    todo_write_ref.send_message(ToolMessage::Execute {
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
            assert!(result.contains("Invalid parameters"));
        }
        _ => panic!("Expected ToolResult message"),
    }
}

#[tokio::test]
async fn test_todo_write_invalid_status() {
    let (config, chat_ref, mut rx) = setup_test().await;
    
    // Create TodoWriteActor
    let todo_write_actor = TodoWriteActor::new(config.clone());
    let (todo_write_ref, _) = Actor::spawn(None, todo_write_actor, config).await.unwrap();
    
    // Send todo with invalid status
    let id = Uuid::new_v4();
    let params = json!({
        "todos": [
            {
                "id": "1",
                "content": "Task with invalid status",
                "status": "invalid_status",
                "priority": "high"
            }
        ]
    });
    
    todo_write_ref.send_message(ToolMessage::Execute {
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
        }
        _ => panic!("Expected ToolResult message"),
    }
}

#[tokio::test]
async fn test_todo_write_missing_fields() {
    let (config, chat_ref, mut rx) = setup_test().await;
    
    // Create TodoWriteActor
    let todo_write_actor = TodoWriteActor::new(config.clone());
    let (todo_write_ref, _) = Actor::spawn(None, todo_write_actor, config).await.unwrap();
    
    // Send todo with missing fields
    let id = Uuid::new_v4();
    let params = json!({
        "todos": [
            {
                "id": "1",
                "content": "Task without status or priority"
            }
        ]
    });
    
    todo_write_ref.send_message(ToolMessage::Execute {
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
        }
        _ => panic!("Expected ToolResult message"),
    }
}

#[tokio::test]
async fn test_todo_write_status_icons() {
    let (config, chat_ref, mut rx) = setup_test().await;
    
    // Create TodoWriteActor
    let todo_write_actor = TodoWriteActor::new(config.clone());
    let (todo_write_ref, _) = Actor::spawn(None, todo_write_actor, config).await.unwrap();
    
    // Send todos with different statuses
    let id = Uuid::new_v4();
    let params = json!({
        "todos": [
            {
                "id": "1",
                "content": "Pending",
                "status": "pending",
                "priority": "high"
            },
            {
                "id": "2",
                "content": "In Progress",
                "status": "in_progress",
                "priority": "high"
            },
            {
                "id": "3",
                "content": "Completed",
                "status": "completed",
                "priority": "high"
            }
        ]
    });
    
    todo_write_ref.send_message(ToolMessage::Execute {
        id,
        params,
        chat_ref,
    }).unwrap();
    
    // Wait for response
    let response = rx.recv().await.unwrap();
    match response {
        ChatMessage::ToolResult { id: res_id, result } => {
            assert_eq!(res_id, id);
            // Check for status icons
            assert!(result.contains("○")); // pending
            assert!(result.contains("◐")); // in_progress
            assert!(result.contains("●")); // completed
        }
        _ => panic!("Expected ToolResult message"),
    }
}