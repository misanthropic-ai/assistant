use assistant_core::actors::tools::{WriteActor, ToolMessage};
use assistant_core::messages::ChatMessage;
use assistant_core::config::Config;
use ractor::{Actor, ActorRef};
use serde_json::json;
use std::fs;
use std::path::Path;
use tempfile::TempDir;
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

async fn setup_test() -> (TempDir, Config, ActorRef<ChatMessage>, mpsc::UnboundedReceiver<ChatMessage>) {
    let temp_dir = TempDir::new().unwrap();
    let config = Config::default();
    
    let (tx, rx) = mpsc::unbounded_channel();
    let mock_chat = MockChatActor { sender: tx.clone() };
    let (chat_ref, _) = Actor::spawn(None, mock_chat, tx).await.unwrap();
    
    (temp_dir, config, chat_ref, rx)
}

#[tokio::test]
async fn test_write_new_file() {
    let (temp_dir, config, chat_ref, mut rx) = setup_test().await;
    
    // Create WriteActor
    let write_actor = WriteActor::new(config.clone());
    let (write_ref, _) = Actor::spawn(None, write_actor, config).await.unwrap();
    
    // Write to a new file
    let id = Uuid::new_v4();
    let file_path = temp_dir.path().join("test_file.txt");
    let content = "Hello, World!\nThis is a test file.";
    let params = json!({
        "file_path": file_path.to_str().unwrap(),
        "content": content
    });
    
    write_ref.send_message(ToolMessage::Execute {
        id,
        params,
        chat_ref,
    }).unwrap();
    
    // Wait for response
    let response = rx.recv().await.unwrap();
    match response {
        ChatMessage::ToolResult { id: res_id, result } => {
            assert_eq!(res_id, id);
            assert!(result.contains("Successfully created new file"));
            assert!(result.contains("Size: 34 bytes"));
            assert!(result.contains("Lines: 2"));
        }
        _ => panic!("Expected ToolResult message"),
    }
    
    // Verify file was actually created
    assert!(file_path.exists());
    let written_content = fs::read_to_string(&file_path).unwrap();
    assert_eq!(written_content, content);
}

#[tokio::test]
async fn test_overwrite_existing_file() {
    let (temp_dir, config, chat_ref, mut rx) = setup_test().await;
    
    // Create an existing file
    let file_path = temp_dir.path().join("existing.txt");
    let original_content = "Original content";
    fs::write(&file_path, original_content).unwrap();
    
    // Create WriteActor
    let write_actor = WriteActor::new(config.clone());
    let (write_ref, _) = Actor::spawn(None, write_actor, config).await.unwrap();
    
    // Overwrite the file
    let id = Uuid::new_v4();
    let new_content = "New content\nWith multiple lines\nAnd more data";
    let params = json!({
        "file_path": file_path.to_str().unwrap(),
        "content": new_content
    });
    
    write_ref.send_message(ToolMessage::Execute {
        id,
        params,
        chat_ref,
    }).unwrap();
    
    // Wait for response
    let response = rx.recv().await.unwrap();
    match response {
        ChatMessage::ToolResult { id: res_id, result } => {
            assert_eq!(res_id, id);
            assert!(result.contains("Successfully overwrote file"));
            assert!(result.contains("Previous size: 16 bytes"));
            assert!(result.contains("New size: 45 bytes"));
            assert!(result.contains("Lines: 3"));
        }
        _ => panic!("Expected ToolResult message"),
    }
    
    // Verify file was overwritten
    let written_content = fs::read_to_string(&file_path).unwrap();
    assert_eq!(written_content, new_content);
}

#[tokio::test]
async fn test_create_parent_directories() {
    let (temp_dir, config, chat_ref, mut rx) = setup_test().await;
    
    // Create WriteActor
    let write_actor = WriteActor::new(config.clone());
    let (write_ref, _) = Actor::spawn(None, write_actor, config).await.unwrap();
    
    // Write to a file in non-existent subdirectories
    let id = Uuid::new_v4();
    let file_path = temp_dir.path().join("subdir1/subdir2/file.txt");
    let content = "File in nested directories";
    let params = json!({
        "file_path": file_path.to_str().unwrap(),
        "content": content
    });
    
    write_ref.send_message(ToolMessage::Execute {
        id,
        params,
        chat_ref,
    }).unwrap();
    
    // Wait for response
    let response = rx.recv().await.unwrap();
    match response {
        ChatMessage::ToolResult { id: res_id, result } => {
            assert_eq!(res_id, id);
            assert!(result.contains("Successfully created new file"));
        }
        _ => panic!("Expected ToolResult message"),
    }
    
    // Verify directories and file were created
    assert!(temp_dir.path().join("subdir1").exists());
    assert!(temp_dir.path().join("subdir1/subdir2").exists());
    assert!(file_path.exists());
    let written_content = fs::read_to_string(&file_path).unwrap();
    assert_eq!(written_content, content);
}

#[tokio::test]
async fn test_write_empty_file() {
    let (temp_dir, config, chat_ref, mut rx) = setup_test().await;
    
    // Create WriteActor
    let write_actor = WriteActor::new(config.clone());
    let (write_ref, _) = Actor::spawn(None, write_actor, config).await.unwrap();
    
    // Write empty content
    let id = Uuid::new_v4();
    let file_path = temp_dir.path().join("empty.txt");
    let params = json!({
        "file_path": file_path.to_str().unwrap(),
        "content": ""
    });
    
    write_ref.send_message(ToolMessage::Execute {
        id,
        params,
        chat_ref,
    }).unwrap();
    
    // Wait for response
    let response = rx.recv().await.unwrap();
    match response {
        ChatMessage::ToolResult { id: res_id, result } => {
            assert_eq!(res_id, id);
            assert!(result.contains("Successfully created new file"));
            assert!(result.contains("Size: 0 bytes"));
            assert!(result.contains("Lines: 0"));
        }
        _ => panic!("Expected ToolResult message"),
    }
    
    // Verify empty file was created
    assert!(file_path.exists());
    let written_content = fs::read_to_string(&file_path).unwrap();
    assert_eq!(written_content, "");
}

#[tokio::test]
async fn test_relative_path_error() {
    let (_temp_dir, config, chat_ref, mut rx) = setup_test().await;
    
    // Create WriteActor
    let write_actor = WriteActor::new(config.clone());
    let (write_ref, _) = Actor::spawn(None, write_actor, config).await.unwrap();
    
    // Try to write with relative path
    let id = Uuid::new_v4();
    let params = json!({
        "file_path": "./relative/path.txt",
        "content": "This should fail"
    });
    
    write_ref.send_message(ToolMessage::Execute {
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
            assert!(result.contains("File path must be absolute"));
        }
        _ => panic!("Expected ToolResult message"),
    }
}

#[tokio::test]
async fn test_write_to_directory_error() {
    let (temp_dir, config, chat_ref, mut rx) = setup_test().await;
    
    // Create a directory
    let dir_path = temp_dir.path().join("a_directory");
    fs::create_dir(&dir_path).unwrap();
    
    // Create WriteActor
    let write_actor = WriteActor::new(config.clone());
    let (write_ref, _) = Actor::spawn(None, write_actor, config).await.unwrap();
    
    // Try to write to a directory
    let id = Uuid::new_v4();
    let params = json!({
        "file_path": dir_path.to_str().unwrap(),
        "content": "This should fail"
    });
    
    write_ref.send_message(ToolMessage::Execute {
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
            assert!(result.contains("Path is a directory, not a file"));
        }
        _ => panic!("Expected ToolResult message"),
    }
}

#[tokio::test]
async fn test_invalid_parameters() {
    let (_temp_dir, config, chat_ref, mut rx) = setup_test().await;
    
    // Create WriteActor
    let write_actor = WriteActor::new(config.clone());
    let (write_ref, _) = Actor::spawn(None, write_actor, config).await.unwrap();
    
    // Send invalid parameters (missing content field)
    let id = Uuid::new_v4();
    let params = json!({
        "file_path": "/tmp/test.txt"
    });
    
    write_ref.send_message(ToolMessage::Execute {
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
async fn test_write_unicode_content() {
    let (temp_dir, config, chat_ref, mut rx) = setup_test().await;
    
    // Create WriteActor
    let write_actor = WriteActor::new(config.clone());
    let (write_ref, _) = Actor::spawn(None, write_actor, config).await.unwrap();
    
    // Write unicode content
    let id = Uuid::new_v4();
    let file_path = temp_dir.path().join("unicode.txt");
    let content = "Hello 世界! 🌍\nСпасибо за внимание\n日本語のテキスト";
    let params = json!({
        "file_path": file_path.to_str().unwrap(),
        "content": content
    });
    
    write_ref.send_message(ToolMessage::Execute {
        id,
        params,
        chat_ref,
    }).unwrap();
    
    // Wait for response
    let response = rx.recv().await.unwrap();
    match response {
        ChatMessage::ToolResult { id: res_id, result } => {
            assert_eq!(res_id, id);
            assert!(result.contains("Successfully created new file"));
            assert!(result.contains("Lines: 3"));
        }
        _ => panic!("Expected ToolResult message"),
    }
    
    // Verify unicode content was written correctly
    let written_content = fs::read_to_string(&file_path).unwrap();
    assert_eq!(written_content, content);
}