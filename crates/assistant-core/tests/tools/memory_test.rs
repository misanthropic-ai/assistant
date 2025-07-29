use assistant_core::actors::tools::{MemoryActor, ToolMessage};
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
async fn test_memory_save_and_load() {
    let (config, chat_ref, mut rx) = setup_test().await;
    
    // Create MemoryActor
    let memory_actor = MemoryActor::new(config.clone());
    let (memory_ref, _) = Actor::spawn(None, memory_actor, config).await.unwrap();
    
    // Save memory
    let save_id = Uuid::new_v4();
    let save_params = json!({
        "operation": "Save",
        "key": "test_key",
        "content": "This is a test memory content"
    });
    
    memory_ref.send_message(ToolMessage::Execute {
        id: save_id,
        params: save_params,
        chat_ref: chat_ref.clone(),
    }).unwrap();
    
    // Wait for save response
    let save_response = rx.recv().await.unwrap();
    match save_response {
        ChatMessage::ToolResult { id, result } => {
            assert_eq!(id, save_id);
            assert!(result.contains("Saved"));
            assert!(result.contains("29 bytes"));
            assert!(result.contains("test_key"));
        }
        _ => panic!("Expected ToolResult message"),
    }
    
    // Load memory
    let load_id = Uuid::new_v4();
    let load_params = json!({
        "operation": "Load",
        "key": "test_key"
    });
    
    memory_ref.send_message(ToolMessage::Execute {
        id: load_id,
        params: load_params,
        chat_ref,
    }).unwrap();
    
    // Wait for load response
    let load_response = rx.recv().await.unwrap();
    match load_response {
        ChatMessage::ToolResult { id, result } => {
            assert_eq!(id, load_id);
            assert_eq!(result, "This is a test memory content");
        }
        _ => panic!("Expected ToolResult message"),
    }
}

#[tokio::test]
async fn test_memory_load_nonexistent() {
    let (config, chat_ref, mut rx) = setup_test().await;
    
    // Create MemoryActor
    let memory_actor = MemoryActor::new(config.clone());
    let (memory_ref, _) = Actor::spawn(None, memory_actor, config).await.unwrap();
    
    // Load non-existent memory
    let id = Uuid::new_v4();
    let params = json!({
        "operation": "Load",
        "key": "nonexistent_key"
    });
    
    memory_ref.send_message(ToolMessage::Execute {
        id,
        params,
        chat_ref,
    }).unwrap();
    
    // Wait for response
    let response = rx.recv().await.unwrap();
    match response {
        ChatMessage::ToolResult { id: res_id, result } => {
            assert_eq!(res_id, id);
            assert!(result.contains("not found"));
            assert!(result.contains("nonexistent_key"));
        }
        _ => panic!("Expected ToolResult message"),
    }
}

#[tokio::test]
async fn test_memory_list() {
    let (config, chat_ref, mut rx) = setup_test().await;
    
    // Create MemoryActor
    let memory_actor = MemoryActor::new(config.clone());
    let (memory_ref, _) = Actor::spawn(None, memory_actor, config).await.unwrap();
    
    // List empty memories
    let list_id = Uuid::new_v4();
    let list_params = json!({
        "operation": "List"
    });
    
    memory_ref.send_message(ToolMessage::Execute {
        id: list_id,
        params: list_params.clone(),
        chat_ref: chat_ref.clone(),
    }).unwrap();
    
    // Wait for response
    let response = rx.recv().await.unwrap();
    match response {
        ChatMessage::ToolResult { id, result } => {
            assert_eq!(id, list_id);
            assert_eq!(result, "No memory keys found");
        }
        _ => panic!("Expected ToolResult message"),
    }
    
    // Save some memories
    let save_params1 = json!({
        "operation": "Save",
        "key": "key1",
        "content": "content1"
    });
    
    memory_ref.send_message(ToolMessage::Execute {
        id: Uuid::new_v4(),
        params: save_params1,
        chat_ref: chat_ref.clone(),
    }).unwrap();
    
    // Consume the save response
    let _ = rx.recv().await.unwrap();
    
    let save_params2 = json!({
        "operation": "Save",
        "key": "key2",
        "content": "content2"
    });
    
    memory_ref.send_message(ToolMessage::Execute {
        id: Uuid::new_v4(),
        params: save_params2,
        chat_ref: chat_ref.clone(),
    }).unwrap();
    
    // Consume the save response
    let _ = rx.recv().await.unwrap();
    
    // List again
    let list_id2 = Uuid::new_v4();
    memory_ref.send_message(ToolMessage::Execute {
        id: list_id2,
        params: list_params,
        chat_ref,
    }).unwrap();
    
    // Wait for response
    let response = rx.recv().await.unwrap();
    match response {
        ChatMessage::ToolResult { id, result } => {
            assert_eq!(id, list_id2);
            assert!(result.contains("Memory keys:"));
            assert!(result.contains("key1"));
            assert!(result.contains("key2"));
        }
        _ => panic!("Expected ToolResult message"),
    }
}

#[tokio::test]
async fn test_memory_clear_specific() {
    let (config, chat_ref, mut rx) = setup_test().await;
    
    // Create MemoryActor
    let memory_actor = MemoryActor::new(config.clone());
    let (memory_ref, _) = Actor::spawn(None, memory_actor, config).await.unwrap();
    
    // Save memories
    let save_params1 = json!({
        "operation": "Save",
        "key": "key1",
        "content": "content1"
    });
    
    memory_ref.send_message(ToolMessage::Execute {
        id: Uuid::new_v4(),
        params: save_params1,
        chat_ref: chat_ref.clone(),
    }).unwrap();
    
    // Consume the save response
    let _ = rx.recv().await.unwrap();
    
    let save_params2 = json!({
        "operation": "Save",
        "key": "key2",
        "content": "content2"
    });
    
    memory_ref.send_message(ToolMessage::Execute {
        id: Uuid::new_v4(),
        params: save_params2,
        chat_ref: chat_ref.clone(),
    }).unwrap();
    
    // Consume the save response
    let _ = rx.recv().await.unwrap();
    
    // Clear specific key
    let clear_id = Uuid::new_v4();
    let clear_params = json!({
        "operation": "Clear",
        "key": "key1"
    });
    
    memory_ref.send_message(ToolMessage::Execute {
        id: clear_id,
        params: clear_params,
        chat_ref: chat_ref.clone(),
    }).unwrap();
    
    // Wait for response
    let response = rx.recv().await.unwrap();
    match response {
        ChatMessage::ToolResult { id, result } => {
            assert_eq!(id, clear_id);
            assert_eq!(result, "Cleared memory key: key1");
        }
        _ => panic!("Expected ToolResult message"),
    }
    
    // Verify key1 is gone but key2 remains
    let load_params = json!({
        "operation": "Load",
        "key": "key1"
    });
    
    memory_ref.send_message(ToolMessage::Execute {
        id: Uuid::new_v4(),
        params: load_params,
        chat_ref: chat_ref.clone(),
    }).unwrap();
    
    let response = rx.recv().await.unwrap();
    match response {
        ChatMessage::ToolResult { id: _, result } => {
            assert!(result.contains("not found"));
        }
        _ => panic!("Expected ToolResult message"),
    }
    
    // Key2 should still exist
    let load_params2 = json!({
        "operation": "Load",
        "key": "key2"
    });
    
    memory_ref.send_message(ToolMessage::Execute {
        id: Uuid::new_v4(),
        params: load_params2,
        chat_ref,
    }).unwrap();
    
    let response = rx.recv().await.unwrap();
    match response {
        ChatMessage::ToolResult { id: _, result } => {
            assert_eq!(result, "content2");
        }
        _ => panic!("Expected ToolResult message"),
    }
}

#[tokio::test]
async fn test_memory_clear_all() {
    let (config, chat_ref, mut rx) = setup_test().await;
    
    // Create MemoryActor
    let memory_actor = MemoryActor::new(config.clone());
    let (memory_ref, _) = Actor::spawn(None, memory_actor, config).await.unwrap();
    
    // Save memories
    let save_params1 = json!({
        "operation": "Save",
        "key": "key1",
        "content": "content1"
    });
    
    memory_ref.send_message(ToolMessage::Execute {
        id: Uuid::new_v4(),
        params: save_params1,
        chat_ref: chat_ref.clone(),
    }).unwrap();
    
    // Consume the save response
    let _ = rx.recv().await.unwrap();
    
    let save_params2 = json!({
        "operation": "Save",
        "key": "key2",
        "content": "content2"
    });
    
    memory_ref.send_message(ToolMessage::Execute {
        id: Uuid::new_v4(),
        params: save_params2,
        chat_ref: chat_ref.clone(),
    }).unwrap();
    
    // Consume the save response
    let _ = rx.recv().await.unwrap();
    
    // Clear all
    let clear_id = Uuid::new_v4();
    let clear_params = json!({
        "operation": "Clear"
    });
    
    memory_ref.send_message(ToolMessage::Execute {
        id: clear_id,
        params: clear_params,
        chat_ref: chat_ref.clone(),
    }).unwrap();
    
    // Wait for response
    let response = rx.recv().await.unwrap();
    match response {
        ChatMessage::ToolResult { id, result } => {
            assert_eq!(id, clear_id);
            assert_eq!(result, "Cleared all memory");
        }
        _ => panic!("Expected ToolResult message"),
    }
    
    // List should now be empty
    let list_params = json!({
        "operation": "List"
    });
    
    memory_ref.send_message(ToolMessage::Execute {
        id: Uuid::new_v4(),
        params: list_params,
        chat_ref,
    }).unwrap();
    
    let response = rx.recv().await.unwrap();
    match response {
        ChatMessage::ToolResult { id: _, result } => {
            assert_eq!(result, "No memory keys found");
        }
        _ => panic!("Expected ToolResult message"),
    }
}

#[tokio::test]
async fn test_memory_invalid_parameters() {
    let (config, chat_ref, mut rx) = setup_test().await;
    
    // Create MemoryActor
    let memory_actor = MemoryActor::new(config.clone());
    let (memory_ref, _) = Actor::spawn(None, memory_actor, config).await.unwrap();
    
    // Invalid operation
    let id = Uuid::new_v4();
    let params = json!({
        "invalid": "parameters"
    });
    
    memory_ref.send_message(ToolMessage::Execute {
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
async fn test_memory_overwrite() {
    let (config, chat_ref, mut rx) = setup_test().await;
    
    // Create MemoryActor
    let memory_actor = MemoryActor::new(config.clone());
    let (memory_ref, _) = Actor::spawn(None, memory_actor, config).await.unwrap();
    
    // Save initial memory
    let save_params1 = json!({
        "operation": "Save",
        "key": "test_key",
        "content": "initial content"
    });
    
    memory_ref.send_message(ToolMessage::Execute {
        id: Uuid::new_v4(),
        params: save_params1,
        chat_ref: chat_ref.clone(),
    }).unwrap();
    
    // Consume the save response
    let _ = rx.recv().await.unwrap();
    
    // Overwrite with new content
    let save_params2 = json!({
        "operation": "Save",
        "key": "test_key",
        "content": "updated content"
    });
    
    memory_ref.send_message(ToolMessage::Execute {
        id: Uuid::new_v4(),
        params: save_params2,
        chat_ref: chat_ref.clone(),
    }).unwrap();
    
    // Consume the save response
    let _ = rx.recv().await.unwrap();
    
    // Load and verify it was overwritten
    let load_params = json!({
        "operation": "Load",
        "key": "test_key"
    });
    
    memory_ref.send_message(ToolMessage::Execute {
        id: Uuid::new_v4(),
        params: load_params,
        chat_ref,
    }).unwrap();
    
    let response = rx.recv().await.unwrap();
    match response {
        ChatMessage::ToolResult { id: _, result } => {
            assert_eq!(result, "updated content");
        }
        _ => panic!("Expected ToolResult message"),
    }
}

#[tokio::test]
async fn test_memory_empty_key() {
    let (config, chat_ref, mut rx) = setup_test().await;
    
    // Create MemoryActor
    let memory_actor = MemoryActor::new(config.clone());
    let (memory_ref, _) = Actor::spawn(None, memory_actor, config).await.unwrap();
    
    // Save with empty key (should work)
    let save_params = json!({
        "operation": "Save",
        "key": "",
        "content": "content with empty key"
    });
    
    memory_ref.send_message(ToolMessage::Execute {
        id: Uuid::new_v4(),
        params: save_params,
        chat_ref: chat_ref.clone(),
    }).unwrap();
    
    // Should succeed
    let response = rx.recv().await.unwrap();
    match response {
        ChatMessage::ToolResult { id: _, result } => {
            assert!(result.contains("Saved"));
            assert!(result.contains("22 bytes"));
        }
        _ => panic!("Expected ToolResult message"),
    }
    
    // Load with empty key
    let load_params = json!({
        "operation": "Load",
        "key": ""
    });
    
    memory_ref.send_message(ToolMessage::Execute {
        id: Uuid::new_v4(),
        params: load_params,
        chat_ref,
    }).unwrap();
    
    let response = rx.recv().await.unwrap();
    match response {
        ChatMessage::ToolResult { id: _, result } => {
            assert_eq!(result, "content with empty key");
        }
        _ => panic!("Expected ToolResult message"),
    }
}