use assistant_core::actors::tools::{ReadActor, ToolMessage};
use assistant_core::messages::ChatMessage;
use assistant_core::config::Config;
use ractor::{Actor, ActorRef};
use serde_json::json;
use tokio::sync::mpsc;
use uuid::Uuid;
use tempfile::TempDir;
use std::fs::File;
use std::io::Write;

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
async fn test_read_file_success() {
    let (config, chat_ref, mut rx) = setup_test().await;
    let temp_dir = TempDir::new().unwrap();
    
    // Create a test file
    let file_path = temp_dir.path().join("test.txt");
    let mut file = File::create(&file_path).unwrap();
    writeln!(file, "Line 1").unwrap();
    writeln!(file, "Line 2").unwrap();
    writeln!(file, "Line 3").unwrap();
    
    // Create ReadActor
    let read_actor = ReadActor::new(config.clone());
    let (read_ref, _) = Actor::spawn(None, read_actor, config).await.unwrap();
    
    // Read the file
    let id = Uuid::new_v4();
    let params = json!({
        "path": file_path.to_str().unwrap()
    });
    
    read_ref.send_message(ToolMessage::Execute {
        id,
        params,
        chat_ref,
    }).unwrap();
    
    // Wait for response
    let response = rx.recv().await.unwrap();
    match response {
        ChatMessage::ToolResult { id: res_id, result } => {
            assert_eq!(res_id, id);
            assert!(result.contains("Line 1"));
            assert!(result.contains("Line 2"));
            assert!(result.contains("Line 3"));
        }
        _ => panic!("Expected ToolResult message"),
    }
}

#[tokio::test]
async fn test_read_file_with_offset_and_limit() {
    let (config, chat_ref, mut rx) = setup_test().await;
    let temp_dir = TempDir::new().unwrap();
    
    // Create a test file with many lines
    let file_path = temp_dir.path().join("many_lines.txt");
    let mut file = File::create(&file_path).unwrap();
    for i in 1..=10 {
        writeln!(file, "Line {}", i).unwrap();
    }
    
    // Create ReadActor
    let read_actor = ReadActor::new(config.clone());
    let (read_ref, _) = Actor::spawn(None, read_actor, config).await.unwrap();
    
    // Read with offset and limit
    let id = Uuid::new_v4();
    let params = json!({
        "path": file_path.to_str().unwrap(),
        "offset": 2,
        "limit": 3
    });
    
    read_ref.send_message(ToolMessage::Execute {
        id,
        params,
        chat_ref,
    }).unwrap();
    
    // Wait for response
    let response = rx.recv().await.unwrap();
    match response {
        ChatMessage::ToolResult { id: res_id, result } => {
            assert_eq!(res_id, id);
            // Should contain lines 3, 4, 5 (offset 2 means skip first 2 lines)
            assert!(result.contains("Line 3"));
            assert!(result.contains("Line 4"));
            assert!(result.contains("Line 5"));
            // Should not contain lines 1, 2, 6+
            assert!(!result.contains("Line 1"));
            assert!(!result.contains("Line 2"));
            assert!(!result.contains("Line 6"));
            // Check line numbers are formatted correctly
            assert!(result.contains("    3│"));
            assert!(result.contains("    4│"));
            assert!(result.contains("    5│"));
        }
        _ => panic!("Expected ToolResult message"),
    }
}

#[tokio::test]
async fn test_read_nonexistent_file() {
    let (config, chat_ref, mut rx) = setup_test().await;
    
    // Create ReadActor
    let read_actor = ReadActor::new(config.clone());
    let (read_ref, _) = Actor::spawn(None, read_actor, config).await.unwrap();
    
    // Try to read nonexistent file
    let id = Uuid::new_v4();
    let params = json!({
        "path": "/nonexistent/file.txt"
    });
    
    read_ref.send_message(ToolMessage::Execute {
        id,
        params,
        chat_ref,
    }).unwrap();
    
    // Wait for response
    let response = rx.recv().await.unwrap();
    match response {
        ChatMessage::ToolResult { id: res_id, result } => {
            assert_eq!(res_id, id);
            assert!(result.contains("Error reading file"));
            assert!(result.contains("/nonexistent/file.txt"));
        }
        _ => panic!("Expected ToolResult message"),
    }
}

#[tokio::test]
async fn test_read_relative_path() {
    let (config, chat_ref, mut rx) = setup_test().await;
    
    // Create ReadActor
    let read_actor = ReadActor::new(config.clone());
    let (read_ref, _) = Actor::spawn(None, read_actor, config).await.unwrap();
    
    // Try to read with relative path
    let id = Uuid::new_v4();
    let params = json!({
        "path": "relative/path.txt"
    });
    
    read_ref.send_message(ToolMessage::Execute {
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
            assert!(result.contains("must be absolute"));
            assert!(result.contains("relative"));
        }
        _ => panic!("Expected ToolResult message"),
    }
}

#[tokio::test]
async fn test_read_empty_file() {
    let (config, chat_ref, mut rx) = setup_test().await;
    let temp_dir = TempDir::new().unwrap();
    
    // Create an empty file
    let file_path = temp_dir.path().join("empty.txt");
    File::create(&file_path).unwrap();
    
    // Create ReadActor
    let read_actor = ReadActor::new(config.clone());
    let (read_ref, _) = Actor::spawn(None, read_actor, config).await.unwrap();
    
    // Read the empty file
    let id = Uuid::new_v4();
    let params = json!({
        "path": file_path.to_str().unwrap()
    });
    
    read_ref.send_message(ToolMessage::Execute {
        id,
        params,
        chat_ref,
    }).unwrap();
    
    // Wait for response
    let response = rx.recv().await.unwrap();
    match response {
        ChatMessage::ToolResult { id: res_id, result } => {
            assert_eq!(res_id, id);
            assert_eq!(result, "");
        }
        _ => panic!("Expected ToolResult message"),
    }
}

#[tokio::test]
async fn test_read_invalid_parameters() {
    let (config, chat_ref, mut rx) = setup_test().await;
    
    // Create ReadActor
    let read_actor = ReadActor::new(config.clone());
    let (read_ref, _) = Actor::spawn(None, read_actor, config).await.unwrap();
    
    // Send invalid parameters
    let id = Uuid::new_v4();
    let params = json!({
        "invalid": "parameters"
    });
    
    read_ref.send_message(ToolMessage::Execute {
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
async fn test_read_directory() {
    let (config, chat_ref, mut rx) = setup_test().await;
    let temp_dir = TempDir::new().unwrap();
    
    // Create ReadActor
    let read_actor = ReadActor::new(config.clone());
    let (read_ref, _) = Actor::spawn(None, read_actor, config).await.unwrap();
    
    // Try to read a directory
    let id = Uuid::new_v4();
    let params = json!({
        "path": temp_dir.path().to_str().unwrap()
    });
    
    read_ref.send_message(ToolMessage::Execute {
        id,
        params,
        chat_ref,
    }).unwrap();
    
    // Wait for response
    let response = rx.recv().await.unwrap();
    match response {
        ChatMessage::ToolResult { id: res_id, result } => {
            assert_eq!(res_id, id);
            assert!(result.contains("Error reading file"));
            // The exact error message depends on the OS
        }
        _ => panic!("Expected ToolResult message"),
    }
}

#[tokio::test]
async fn test_read_with_offset_beyond_file() {
    let (config, chat_ref, mut rx) = setup_test().await;
    let temp_dir = TempDir::new().unwrap();
    
    // Create a small file
    let file_path = temp_dir.path().join("small.txt");
    let mut file = File::create(&file_path).unwrap();
    writeln!(file, "Line 1").unwrap();
    writeln!(file, "Line 2").unwrap();
    
    // Create ReadActor
    let read_actor = ReadActor::new(config.clone());
    let (read_ref, _) = Actor::spawn(None, read_actor, config).await.unwrap();
    
    // Read with offset beyond file
    let id = Uuid::new_v4();
    let params = json!({
        "path": file_path.to_str().unwrap(),
        "offset": 10
    });
    
    read_ref.send_message(ToolMessage::Execute {
        id,
        params,
        chat_ref,
    }).unwrap();
    
    // Wait for response
    let response = rx.recv().await.unwrap();
    match response {
        ChatMessage::ToolResult { id: res_id, result } => {
            assert_eq!(res_id, id);
            // Should return empty when offset is beyond file
            assert_eq!(result, "");
        }
        _ => panic!("Expected ToolResult message"),
    }
}