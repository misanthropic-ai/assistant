use assistant_core::{
    actors::tools::memory::MemoryActor,
    config::Config,
    messages::{ToolMessage, ChatMessage},
    ractor::{Actor, ActorRef},
};
use serde_json::json;
use tempfile::TempDir;
use tokio::sync::mpsc;
use uuid::Uuid;

struct TestSetup {
    memory_ref: ActorRef<ToolMessage>,
    chat_ref: ActorRef<ChatMessage>,
    rx: mpsc::UnboundedReceiver<ChatMessage>,
    _temp_dir: TempDir,
}

async fn setup_memory_test() -> TestSetup {
    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path().join("memory.db");
    
    let mut config = Config::default();
    config.api_key = "test-api-key".to_string();
    config.session.database_path = Some(db_path);
    // Configure embeddings
    if let Some(model_config) = config.embeddings.models.get_mut("openai-small") {
        model_config.api_key = Some("test-api-key".to_string());
    }
    
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
    
    // Create memory actor
    let memory_actor = MemoryActor::new(config.clone())
        .await
        .expect("Failed to create memory actor");
    
    let (memory_ref, _) = Actor::spawn(
        None,
        memory_actor,
        config,
    )
    .await
    .expect("Failed to spawn memory actor");
    
    TestSetup {
        memory_ref,
        chat_ref,
        rx,
        _temp_dir: temp_dir,
    }
}

#[tokio::test]
async fn test_store_and_retrieve_memory() {
    let mut setup = setup_memory_test().await;
    
    // Store a memory with auto-generated key
    let store_id = Uuid::new_v4();
    let params = json!({
        "action": "store",
        "content": "My favorite color is blue",
        "metadata": {
            "category": "personal_preference",
            "type": "color"
        }
    });
    
    setup.memory_ref
        .send_message(ToolMessage::Execute {
            id: store_id,
            params,
            chat_ref: setup.chat_ref.clone(),
        })
        .expect("Failed to send store message");
    
    // Get the response
    let response = setup.rx.recv().await.expect("Failed to receive response");
    let stored_key = match response {
        ChatMessage::ToolResult { id, result } => {
            assert_eq!(id, store_id);
            assert!(result.contains("Stored memory with key:"));
            // Extract the key from the response
            result.split("key: ").nth(1).unwrap().trim().to_string()
        }
        _ => panic!("Expected ToolResult"),
    };
    
    // Retrieve the memory
    let retrieve_id = Uuid::new_v4();
    let params = json!({
        "action": "retrieve",
        "key": stored_key
    });
    
    setup.memory_ref
        .send_message(ToolMessage::Execute {
            id: retrieve_id,
            params,
            chat_ref: setup.chat_ref.clone(),
        })
        .expect("Failed to send retrieve message");
    
    let response = setup.rx.recv().await.expect("Failed to receive response");
    match response {
        ChatMessage::ToolResult { id, result } => {
            assert_eq!(id, retrieve_id);
            assert!(result.contains("My favorite color is blue"));
            assert!(result.contains("Metadata:"));
            assert!(result.contains("personal_preference"));
            assert!(result.contains("color"));
        }
        _ => panic!("Expected ToolResult"),
    }
}

#[tokio::test]
async fn test_store_with_key() {
    let mut setup = setup_memory_test().await;
    
    // Store with specific key
    let store_id = Uuid::new_v4();
    let params = json!({
        "action": "store_with_key",
        "key": "user_preferences",
        "content": "Likes pizza and coffee",
        "metadata": {
            "category": "food"
        }
    });
    
    setup.memory_ref
        .send_message(ToolMessage::Execute {
            id: store_id,
            params,
            chat_ref: setup.chat_ref.clone(),
        })
        .expect("Failed to send store message");
    
    let response = setup.rx.recv().await.expect("Failed to receive response");
    match response {
        ChatMessage::ToolResult { id, result } => {
            assert_eq!(id, store_id);
            assert_eq!(result, "Stored memory with key: user_preferences");
        }
        _ => panic!("Expected ToolResult"),
    }
}

#[tokio::test]
#[ignore = "Requires real OpenAI API key for embeddings in hybrid search"]
async fn test_search_memories() {
    let mut setup = setup_memory_test().await;
    
    // Store multiple memories
    let memories = vec![
        ("weather_pref", "I love sunny days", json!({"category": "weather"})),
        ("food_pref", "Pizza is my favorite food", json!({"category": "food"})),
        ("color_pref", "Blue skies are beautiful", json!({"category": "color"})),
    ];
    
    for (key, content, metadata) in memories {
        let params = json!({
            "action": "store_with_key",
            "key": key,
            "content": content,
            "metadata": metadata
        });
        
        setup.memory_ref
            .send_message(ToolMessage::Execute {
                id: Uuid::new_v4(),
                params,
                chat_ref: setup.chat_ref.clone(),
            })
            .expect("Failed to send store message");
        
        // Consume response
        let _ = setup.rx.recv().await;
    }
    
    // Add small delay to ensure FTS5 indexing completes
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    
    // Search for "blue" (FTS5 should be case-insensitive)
    let search_id = Uuid::new_v4();
    let params = json!({
        "action": "search",
        "query": "blue",
        "limit": 5,
        "mode": "hybrid"
    });
    
    setup.memory_ref
        .send_message(ToolMessage::Execute {
            id: search_id,
            params,
            chat_ref: setup.chat_ref.clone(),
        })
        .expect("Failed to send search message");
    
    let response = setup.rx.recv().await.expect("Failed to receive response");
    match response {
        ChatMessage::ToolResult { id, result } => {
            assert_eq!(id, search_id);
            println!("Search result: {}", result);
            // Should find the "Blue skies" memory
            assert!(result.contains("Found") || result.contains("found"));
            assert!(result.contains("memories"));
            assert!(result.contains("Blue skies") || result.contains("beautiful"));
        }
        _ => panic!("Expected ToolResult"),
    }
}

#[tokio::test]
async fn test_update_memory() {
    let mut setup = setup_memory_test().await;
    
    // Store initial memory
    let params = json!({
        "action": "store_with_key",
        "key": "test_update",
        "content": "Initial content",
        "metadata": {
            "version": 1
        }
    });
    
    setup.memory_ref
        .send_message(ToolMessage::Execute {
            id: Uuid::new_v4(),
            params,
            chat_ref: setup.chat_ref.clone(),
        })
        .expect("Failed to send store message");
    
    let _ = setup.rx.recv().await;
    
    // Update the memory
    let update_id = Uuid::new_v4();
    let params = json!({
        "action": "update",
        "key": "test_update",
        "content": "Updated content",
        "metadata": {
            "version": 2,
            "updated": true
        },
        "merge_metadata": true
    });
    
    setup.memory_ref
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
            assert_eq!(result, "Updated memory: test_update");
        }
        _ => panic!("Expected ToolResult"),
    }
    
    // Retrieve to verify update
    let params = json!({
        "action": "retrieve",
        "key": "test_update"
    });
    
    setup.memory_ref
        .send_message(ToolMessage::Execute {
            id: Uuid::new_v4(),
            params,
            chat_ref: setup.chat_ref.clone(),
        })
        .expect("Failed to send retrieve message");
    
    let response = setup.rx.recv().await.expect("Failed to receive response");
    match response {
        ChatMessage::ToolResult { result, .. } => {
            assert!(result.contains("Updated content"));
            assert!(result.contains("Metadata:"));
            assert!(result.contains("version"));
            assert!(result.contains("2"));
            assert!(result.contains("updated"));
            assert!(result.contains("true"));
        }
        _ => panic!("Expected ToolResult"),
    }
}

#[tokio::test]
async fn test_delete_memory() {
    let mut setup = setup_memory_test().await;
    
    // Store a memory
    let params = json!({
        "action": "store_with_key",
        "key": "to_delete",
        "content": "This will be deleted"
    });
    
    setup.memory_ref
        .send_message(ToolMessage::Execute {
            id: Uuid::new_v4(),
            params,
            chat_ref: setup.chat_ref.clone(),
        })
        .expect("Failed to send store message");
    
    let _ = setup.rx.recv().await;
    
    // Delete it
    let delete_id = Uuid::new_v4();
    let params = json!({
        "action": "delete",
        "key": "to_delete"
    });
    
    setup.memory_ref
        .send_message(ToolMessage::Execute {
            id: delete_id,
            params,
            chat_ref: setup.chat_ref.clone(),
        })
        .expect("Failed to send delete message");
    
    let response = setup.rx.recv().await.expect("Failed to receive response");
    match response {
        ChatMessage::ToolResult { id, result } => {
            assert_eq!(id, delete_id);
            assert_eq!(result, "Deleted memory: to_delete");
        }
        _ => panic!("Expected ToolResult"),
    }
    
    // Try to retrieve - should fail
    let params = json!({
        "action": "retrieve",
        "key": "to_delete"
    });
    
    setup.memory_ref
        .send_message(ToolMessage::Execute {
            id: Uuid::new_v4(),
            params,
            chat_ref: setup.chat_ref.clone(),
        })
        .expect("Failed to send retrieve message");
    
    let response = setup.rx.recv().await.expect("Failed to receive response");
    match response {
        ChatMessage::ToolResult { result, .. } => {
            assert!(result.contains("Memory key") && result.contains("not found"));
        }
        _ => panic!("Expected ToolResult"),
    }
}

#[tokio::test]
async fn test_list_memories() {
    let mut setup = setup_memory_test().await;
    
    // Store some memories
    for i in 0..3 {
        let params = json!({
            "action": "store_with_key",
            "key": format!("test_key_{}", i),
            "content": format!("Content {}", i)
        });
        
        setup.memory_ref
            .send_message(ToolMessage::Execute {
                id: Uuid::new_v4(),
                params,
                chat_ref: setup.chat_ref.clone(),
            })
            .expect("Failed to send store message");
        
        let _ = setup.rx.recv().await;
    }
    
    // List all
    let list_id = Uuid::new_v4();
    let params = json!({
        "action": "list"
    });
    
    setup.memory_ref
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
            assert!(result.contains("Memory keys (3):"));
            assert!(result.contains("test_key_0"));
            assert!(result.contains("test_key_1"));
            assert!(result.contains("test_key_2"));
        }
        _ => panic!("Expected ToolResult"),
    }
}

#[tokio::test]
async fn test_memory_stats() {
    let mut setup = setup_memory_test().await;
    
    // Get initial stats
    let stats_id = Uuid::new_v4();
    let params = json!({
        "action": "stats"
    });
    
    setup.memory_ref
        .send_message(ToolMessage::Execute {
            id: stats_id,
            params: params.clone(),
            chat_ref: setup.chat_ref.clone(),
        })
        .expect("Failed to send stats message");
    
    let response = setup.rx.recv().await.expect("Failed to receive response");
    match response {
        ChatMessage::ToolResult { id, result } => {
            assert_eq!(id, stats_id);
            assert!(result.contains("Memory Statistics:"));
            assert!(result.contains("Total memories: 0"));
        }
        _ => panic!("Expected ToolResult"),
    }
    
    // Add some memories
    for i in 0..5 {
        let params = json!({
            "action": "store",
            "content": format!("Memory {}", i)
        });
        
        setup.memory_ref
            .send_message(ToolMessage::Execute {
                id: Uuid::new_v4(),
                params,
                chat_ref: setup.chat_ref.clone(),
            })
            .expect("Failed to send store message");
        
        let _ = setup.rx.recv().await;
    }
    
    // Get stats again
    setup.memory_ref
        .send_message(ToolMessage::Execute {
            id: Uuid::new_v4(),
            params,
            chat_ref: setup.chat_ref.clone(),
        })
        .expect("Failed to send stats message");
    
    let response = setup.rx.recv().await.expect("Failed to receive response");
    match response {
        ChatMessage::ToolResult { result, .. } => {
            assert!(result.contains("Memory Statistics:"));
            assert!(result.contains("Total memories: 5"));
            assert!(result.contains("Total size:"));
        }
        _ => panic!("Expected ToolResult"),
    }
}

#[tokio::test]
async fn test_clear_memories() {
    let mut setup = setup_memory_test().await;
    
    // Store some memories
    for i in 0..3 {
        let params = json!({
            "action": "store",
            "content": format!("Memory {}", i)
        });
        
        setup.memory_ref
            .send_message(ToolMessage::Execute {
                id: Uuid::new_v4(),
                params,
                chat_ref: setup.chat_ref.clone(),
            })
            .expect("Failed to send store message");
        
        let _ = setup.rx.recv().await;
    }
    
    // Clear all
    let clear_id = Uuid::new_v4();
    let params = json!({
        "action": "clear"
    });
    
    setup.memory_ref
        .send_message(ToolMessage::Execute {
            id: clear_id,
            params,
            chat_ref: setup.chat_ref.clone(),
        })
        .expect("Failed to send clear message");
    
    let response = setup.rx.recv().await.expect("Failed to receive response");
    match response {
        ChatMessage::ToolResult { id, result } => {
            assert_eq!(id, clear_id);
            // The result should indicate cleared memories
            assert!(result.contains("Cleared") && result.contains("memories"));
        }
        _ => panic!("Expected ToolResult"),
    }
    
    // Verify all are gone
    let params = json!({
        "action": "stats"
    });
    
    setup.memory_ref
        .send_message(ToolMessage::Execute {
            id: Uuid::new_v4(),
            params,
            chat_ref: setup.chat_ref.clone(),
        })
        .expect("Failed to send stats message");
    
    let response = setup.rx.recv().await.expect("Failed to receive response");
    match response {
        ChatMessage::ToolResult { result, .. } => {
            assert!(result.contains("Memory Statistics:"));
            assert!(result.contains("Total memories: 0"));
        }
        _ => panic!("Expected ToolResult"),
    }
}

#[tokio::test]
#[ignore = "Requires real OpenAI API key for embeddings in hybrid search"]
async fn test_metadata_filtering() {
    let mut setup = setup_memory_test().await;
    
    // Store memories with different metadata
    let memories = vec![
        ("mem1", "Work meeting notes", json!({"type": "work", "priority": "high"})),
        ("mem2", "Personal diary entry", json!({"type": "personal", "priority": "low"})),
        ("mem3", "Project deadline", json!({"type": "work", "priority": "high"})),
    ];
    
    for (key, content, metadata) in memories {
        let params = json!({
            "action": "store_with_key",
            "key": key,
            "content": content,
            "metadata": metadata
        });
        
        setup.memory_ref
            .send_message(ToolMessage::Execute {
                id: Uuid::new_v4(),
                params,
                chat_ref: setup.chat_ref.clone(),
            })
            .expect("Failed to send store message");
        
        let _ = setup.rx.recv().await;
    }
    
    // Search with metadata filter
    let search_id = Uuid::new_v4();
    let params = json!({
        "action": "search",
        "query": "notes deadline",
        "metadata_filter": {
            "type": "work"
        }
    });
    
    setup.memory_ref
        .send_message(ToolMessage::Execute {
            id: search_id,
            params,
            chat_ref: setup.chat_ref.clone(),
        })
        .expect("Failed to send search message");
    
    let response = setup.rx.recv().await.expect("Failed to receive response");
    match response {
        ChatMessage::ToolResult { id, result } => {
            assert_eq!(id, search_id);
            // Should only find work-related memories
            assert!(result.contains("Found 2 memories:"));
            assert!(result.contains("work"));
            assert!(result.contains("Meeting notes"));
            assert!(result.contains("Project deadline"));
        }
        _ => panic!("Expected ToolResult"),
    }
}