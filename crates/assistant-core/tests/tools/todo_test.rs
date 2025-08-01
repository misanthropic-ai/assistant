use assistant_core::{
    actors::tools::todo::TodoActor,
    config::Config,
    messages::{ToolMessage, ChatMessage},
    ractor::{Actor, ActorRef},
};
use serde_json::json;
use tempfile::TempDir;
use tokio::sync::mpsc;
use uuid::Uuid;

struct TestSetup {
    todo_ref: ActorRef<ToolMessage>,
    chat_ref: ActorRef<ChatMessage>,
    rx: mpsc::UnboundedReceiver<ChatMessage>,
    _temp_dir: TempDir,
}

async fn setup_todo_test() -> TestSetup {
    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path().join("todos.db");
    
    let mut config = Config::default();
    config.api_key = "test-api-key".to_string();
    config.session.database_path = Some(db_path);
    config.session.session_id = Some(Uuid::new_v4().to_string());
    
    // Create a channel to receive responses
    let (tx, rx) = mpsc::unbounded_channel();
    
    // Create mock chat actor
    struct MockChatActor {
        tx: mpsc::UnboundedSender<ChatMessage>,
    }
    
    impl Actor for MockChatActor {
        type Msg = ChatMessage;
        type State = ();
        type Arguments = mpsc::UnboundedSender<ChatMessage>;
        
        async fn pre_start(
            &self,
            _myself: ActorRef<Self::Msg>,
            _tx: Self::Arguments,
        ) -> Result<Self::State, assistant_core::ractor::ActorProcessingErr> {
            Ok(())
        }
        
        async fn handle(
            &self,
            _myself: ActorRef<Self::Msg>,
            msg: Self::Msg,
            _state: &mut Self::State,
        ) -> Result<(), assistant_core::ractor::ActorProcessingErr> {
            let _ = self.tx.send(msg);
            Ok(())
        }
    }
    
    let mock_chat = MockChatActor { tx: tx.clone() };
    let (chat_ref, _) = Actor::spawn(
        None,
        mock_chat,
        tx,
    )
    .await
    .expect("Failed to spawn mock chat");
    
    // Create todo actor
    let todo_actor = TodoActor::new(config.clone())
        .await
        .expect("Failed to create todo actor");
    
    let (todo_ref, _) = Actor::spawn(
        None,
        todo_actor,
        config,
    )
    .await
    .expect("Failed to spawn todo actor");
    
    TestSetup {
        todo_ref,
        chat_ref,
        rx,
        _temp_dir: temp_dir,
    }
}

#[tokio::test]
async fn test_add_and_list_todos() {
    let mut setup = setup_todo_test().await;
    
    // Add a todo
    let add_id = Uuid::new_v4();
    let params = json!({
        "operation": "add",
        "session_id": "default",
        "content": "Write unit tests",
        "priority": "high"
    });
    
    setup.todo_ref
        .send_message(ToolMessage::Execute {
            id: add_id,
            params,
            chat_ref: setup.chat_ref.clone(),
        })
        .expect("Failed to send add message");
    
    // Get the response
    let response = setup.rx.recv().await.expect("Failed to receive response");
    match response {
        ChatMessage::ToolResult { id, result } => {
            assert_eq!(id, add_id);
            assert!(result.contains("Added todo #"));
        }
        _ => panic!("Expected ToolResult"),
    }
    
    // List todos
    let list_id = Uuid::new_v4();
    let params = json!({
        "operation": "list",
        "session_id": "default"
    });
    
    setup.todo_ref
        .send_message(ToolMessage::Execute {
            id: list_id,
            params,
            chat_ref: setup.chat_ref.clone(),
        })
        .expect("Failed to send list message");
    
    let response = setup.rx.recv().await.expect("Failed to receive response");
    match response {
        ChatMessage::ToolResult { id, result } => {
            assert_eq!(id, list_id);
            assert!(result.contains("Write unit tests"));
            assert!(result.contains("pending"));
            assert!(result.contains("high"));
        }
        _ => panic!("Expected ToolResult"),
    }
}

#[tokio::test]
async fn test_update_todo_status() {
    let mut setup = setup_todo_test().await;
    
    // Add a todo
    let params = json!({
        "operation": "add",
        "session_id": "default",
        "content": "Test task",
        "priority": "medium"
    });
    
    setup.todo_ref
        .send_message(ToolMessage::Execute {
            id: Uuid::new_v4(),
            params,
            chat_ref: setup.chat_ref.clone(),
        })
        .expect("Failed to send add message");
    
    let response = setup.rx.recv().await.expect("Failed to receive response");
    let todo_id = match response {
        ChatMessage::ToolResult { result, .. } => {
            // Extract ID from result, e.g. "Added todo #abc123: content"
            result.split("Added todo #").nth(1)
                .and_then(|s| s.split(':').next())
                .unwrap_or("test-1").trim().to_string()
        }
        _ => panic!("Expected ToolResult"),
    };
    
    // Update status to in_progress
    let update_id = Uuid::new_v4();
    let params = json!({
        "operation": "update",
        "session_id": "default",
        "id": todo_id.clone(),
        "status": "in_progress"
    });
    
    setup.todo_ref
        .send_message(ToolMessage::Execute {
            id: update_id,
            params,
            chat_ref: setup.chat_ref.clone(),
        })
        .expect("Failed to send update message");
    
    let response = setup.rx.recv().await.expect("Failed to receive response");
    match response {
        ChatMessage::ToolResult { id, result } => {
            assert_eq!(id, update_id);
            assert!(result.contains("in_progress"));
        }
        _ => panic!("Expected ToolResult"),
    }
    
    // Update to completed
    let complete_id = Uuid::new_v4();
    let params = json!({
        "operation": "update",
        "session_id": "default",
        "id": todo_id,
        "status": "completed"
    });
    
    setup.todo_ref
        .send_message(ToolMessage::Execute {
            id: complete_id,
            params,
            chat_ref: setup.chat_ref.clone(),
        })
        .expect("Failed to send complete message");
    
    let response = setup.rx.recv().await.expect("Failed to receive response");
    match response {
        ChatMessage::ToolResult { id, result } => {
            assert_eq!(id, complete_id);
            assert!(result.contains("completed"));
        }
        _ => panic!("Expected ToolResult"),
    }
}

#[tokio::test]
async fn test_multiple_todos_with_priorities() {
    let mut setup = setup_todo_test().await;
    
    // Add multiple todos with different priorities
    // Add high priority todo
    let add_id = Uuid::new_v4();
    let params = json!({
        "operation": "add",
        "session_id": "default",
        "content": "Critical bug fix",
        "priority": "high"
    });
    
    setup.todo_ref
        .send_message(ToolMessage::Execute {
            id: add_id,
            params,
            chat_ref: setup.chat_ref.clone(),
        })
        .expect("Failed to send add message");
    let _ = setup.rx.recv().await;
    
    // Add low priority todo
    let params = json!({
        "operation": "add",
        "session_id": "default",
        "content": "Documentation update",
        "priority": "low"
    });
    setup.todo_ref
        .send_message(ToolMessage::Execute {
            id: Uuid::new_v4(),
            params,
            chat_ref: setup.chat_ref.clone(),
        })
        .expect("Failed to send add message");
    let _ = setup.rx.recv().await;
    
    // Add medium priority todo
    let params = json!({
        "operation": "add",
        "session_id": "default",
        "content": "Code refactoring",
        "priority": "medium"
    });
    setup.todo_ref
        .send_message(ToolMessage::Execute {
            id: Uuid::new_v4(),
            params,
            chat_ref: setup.chat_ref.clone(),
        })
        .expect("Failed to send add message");
    let _ = setup.rx.recv().await;
    
    // List todos to verify priority ordering
    let list_id = Uuid::new_v4();
    let params = json!({
        "operation": "list",
        "session_id": "default"
    });
    setup.todo_ref
        .send_message(ToolMessage::Execute {
            id: list_id,
            params,
            chat_ref: setup.chat_ref.clone(),
        })
        .expect("Failed to send list message");
    
    let response = setup.rx.recv().await.expect("Failed to receive response");
    match response {
        ChatMessage::ToolResult { id, result } => {
            assert_eq!(id, list_id);
            // High priority should appear first
            let high_pos = result.find("Critical bug fix").unwrap();
            let medium_pos = result.find("Code refactoring").unwrap();
            let low_pos = result.find("Documentation update").unwrap();
            
            assert!(high_pos < medium_pos, "High priority should come before medium");
            assert!(medium_pos < low_pos, "Medium priority should come before low");
        }
        _ => panic!("Expected ToolResult"),
    }
}

#[tokio::test]
async fn test_remove_todo() {
    let mut setup = setup_todo_test().await;
    
    // Add first todo
    let params = json!({
        "operation": "add",
        "session_id": "default",
        "content": "Keep this task",
        "priority": "high"
    });
    
    setup.todo_ref
        .send_message(ToolMessage::Execute {
            id: Uuid::new_v4(),
            params,
            chat_ref: setup.chat_ref.clone(),
        })
        .expect("Failed to send add message");
    
    let response = setup.rx.recv().await.expect("Failed to receive response");
    let _keep_id = match response {
        ChatMessage::ToolResult { result, .. } => {
            result.split("Added todo #").nth(1)
                .and_then(|s| s.split(':').next())
                .unwrap_or("keep-1").trim().to_string()
        }
        _ => panic!("Expected ToolResult"),
    };
    
    // Add second todo to remove
    let params = json!({
        "operation": "add",
        "session_id": "default",
        "content": "Remove this task",
        "priority": "low"
    });
    setup.todo_ref
        .send_message(ToolMessage::Execute {
            id: Uuid::new_v4(),
            params,
            chat_ref: setup.chat_ref.clone(),
        })
        .expect("Failed to send add message");
    
    let response = setup.rx.recv().await.expect("Failed to receive response");
    let remove_todo_id = match response {
        ChatMessage::ToolResult { result, .. } => {
            result.split("Added todo #").nth(1)
                .and_then(|s| s.split(':').next())
                .unwrap_or("remove-1").trim().to_string()
        }
        _ => panic!("Expected ToolResult"),
    };
    
    // Remove the second todo
    let remove_id = Uuid::new_v4();
    let params = json!({
        "operation": "remove",
        "session_id": "default",
        "id": remove_todo_id
    });
    
    setup.todo_ref
        .send_message(ToolMessage::Execute {
            id: remove_id,
            params,
            chat_ref: setup.chat_ref.clone(),
        })
        .expect("Failed to send remove message");
    
    let response = setup.rx.recv().await.expect("Failed to receive response");
    match response {
        ChatMessage::ToolResult { id, result } => {
            assert_eq!(id, remove_id);
            assert!(result.contains("Keep this task"));
            assert!(!result.contains("Remove this task"));
        }
        _ => panic!("Expected ToolResult"),
    }
}

#[tokio::test]
async fn test_empty_todo_list() {
    let mut setup = setup_todo_test().await;
    
    // Get empty list
    let list_id = Uuid::new_v4();
    let params = json!({
        "operation": "list",
        "session_id": "default"
    });
    
    setup.todo_ref
        .send_message(ToolMessage::Execute {
            id: list_id,
            params,
            chat_ref: setup.chat_ref.clone(),
        })
        .expect("Failed to send list message");
    
    let response = setup.rx.recv().await.expect("Failed to receive response");
    match response {
        ChatMessage::ToolResult { id, result } => {
            assert_eq!(id, list_id);
            assert!(result.contains("No todos"));
        }
        _ => panic!("Expected ToolResult"),
    }
}

#[tokio::test]
async fn test_todo_persistence_across_sessions() {
    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path().join("todos.db");
    let session_id = Uuid::new_v4().to_string();
    
    // First session - add todos
    {
        let mut config = Config::default();
        config.api_key = "test-api-key".to_string();
        config.session.database_path = Some(db_path.clone());
        config.session.session_id = Some(session_id.clone());
        
        let (tx, mut rx) = mpsc::unbounded_channel();
        
        struct MockChatActor {
            tx: mpsc::UnboundedSender<ChatMessage>,
        }
        
        impl Actor for MockChatActor {
            type Msg = ChatMessage;
            type State = ();
            type Arguments = mpsc::UnboundedSender<ChatMessage>;
            
            async fn pre_start(
                &self,
                _myself: ActorRef<Self::Msg>,
                _tx: Self::Arguments,
            ) -> Result<Self::State, assistant_core::ractor::ActorProcessingErr> {
                Ok(())
            }
            
            async fn handle(
                &self,
                _myself: ActorRef<Self::Msg>,
                msg: Self::Msg,
                _state: &mut Self::State,
            ) -> Result<(), assistant_core::ractor::ActorProcessingErr> {
                let _ = self.tx.send(msg);
                Ok(())
            }
        }
        
        let mock_chat = MockChatActor { tx: tx.clone() };
        let (chat_ref, _) = Actor::spawn(
            None,
            mock_chat,
            tx,
        )
        .await
        .expect("Failed to spawn mock chat");
        
        let todo_actor = TodoActor::new(config.clone())
            .await
            .expect("Failed to create todo actor");
        
        let (todo_ref, _) = Actor::spawn(
            None,
            todo_actor,
            config,
        )
        .await
        .expect("Failed to spawn todo actor");
        
        // Add todos
        let params = json!({
            "operation": "add",
            "session_id": session_id.clone(),
            "content": "Persistent task",
            "priority": "high"
        });
        
        todo_ref
            .send_message(ToolMessage::Execute {
                id: Uuid::new_v4(),
                params,
                chat_ref,
            })
            .expect("Failed to send add message");
        
        let _ = rx.recv().await;
        
        // Give time for persistence to complete
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    }
    
    // Second session - verify todos persist
    {
        let mut config = Config::default();
        config.api_key = "test-api-key".to_string();
        config.session.database_path = Some(db_path);
        config.session.session_id = Some(session_id.clone());
        
        let (tx, mut rx) = mpsc::unbounded_channel();
        
        struct MockChatActor {
            tx: mpsc::UnboundedSender<ChatMessage>,
        }
        
        impl Actor for MockChatActor {
            type Msg = ChatMessage;
            type State = ();
            type Arguments = mpsc::UnboundedSender<ChatMessage>;
            
            async fn pre_start(
                &self,
                _myself: ActorRef<Self::Msg>,
                _tx: Self::Arguments,
            ) -> Result<Self::State, assistant_core::ractor::ActorProcessingErr> {
                Ok(())
            }
            
            async fn handle(
                &self,
                _myself: ActorRef<Self::Msg>,
                msg: Self::Msg,
                _state: &mut Self::State,
            ) -> Result<(), assistant_core::ractor::ActorProcessingErr> {
                let _ = self.tx.send(msg);
                Ok(())
            }
        }
        
        let mock_chat = MockChatActor { tx: tx.clone() };
        let (chat_ref, _) = Actor::spawn(
            None,
            mock_chat,
            tx,
        )
        .await
        .expect("Failed to spawn mock chat");
        
        let todo_actor = TodoActor::new(config.clone())
            .await
            .expect("Failed to create todo actor");
        
        let (todo_ref, _) = Actor::spawn(
            None,
            todo_actor,
            config,
        )
        .await
        .expect("Failed to spawn todo actor");
        
        // List todos
        let params = json!({
            "operation": "list",
            "session_id": session_id.clone()
        });
        
        todo_ref
            .send_message(ToolMessage::Execute {
                id: Uuid::new_v4(),
                params,
                chat_ref,
            })
            .expect("Failed to send list message");
        
        let response = rx.recv().await.expect("Failed to receive response");
        match response {
            ChatMessage::ToolResult { result, .. } => {
                assert!(result.contains("Persistent task"));
                assert!(result.contains("pending"));
                assert!(result.contains("high"));
            }
            _ => panic!("Expected ToolResult"),
        }
    }
}