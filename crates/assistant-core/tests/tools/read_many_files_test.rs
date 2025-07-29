use assistant_core::actors::tools::{ReadManyFilesActor, ToolMessage};
use assistant_core::messages::ChatMessage;
use assistant_core::config::Config;
use ractor::{Actor, ActorRef};
use serde_json::json;
use tokio::sync::mpsc;
use uuid::Uuid;
use tempfile::TempDir;
use std::fs::File;
use std::io::Write;
use std::path::Path;

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

fn create_test_files(temp_dir: &TempDir) -> Vec<String> {
    let mut paths = Vec::new();
    
    // Create file 1
    let file1_path = temp_dir.path().join("file1.txt");
    let mut file1 = File::create(&file1_path).unwrap();
    writeln!(file1, "This is file 1").unwrap();
    writeln!(file1, "It has multiple lines").unwrap();
    writeln!(file1, "Line 3").unwrap();
    paths.push(file1_path.to_str().unwrap().to_string());
    
    // Create file 2
    let file2_path = temp_dir.path().join("file2.txt");
    let mut file2 = File::create(&file2_path).unwrap();
    writeln!(file2, "This is file 2").unwrap();
    writeln!(file2, "Single additional line").unwrap();
    paths.push(file2_path.to_str().unwrap().to_string());
    
    // Create file 3 (empty)
    let file3_path = temp_dir.path().join("empty.txt");
    File::create(&file3_path).unwrap();
    paths.push(file3_path.to_str().unwrap().to_string());
    
    paths
}

#[tokio::test]
async fn test_read_many_files_success() {
    let (config, chat_ref, mut rx) = setup_test().await;
    let temp_dir = TempDir::new().unwrap();
    let file_paths = create_test_files(&temp_dir);
    
    // Create ReadManyFilesActor
    let read_many_actor = ReadManyFilesActor::new(config.clone());
    let (read_many_ref, _) = Actor::spawn(None, read_many_actor, config).await.unwrap();
    
    // Read multiple files
    let id = Uuid::new_v4();
    let params = json!({
        "paths": file_paths
    });
    
    read_many_ref.send_message(ToolMessage::Execute {
        id,
        params,
        chat_ref,
    }).unwrap();
    
    // Wait for response
    let response = rx.recv().await.unwrap();
    match response {
        ChatMessage::ToolResult { id: res_id, result } => {
            assert_eq!(res_id, id);
            assert!(result.contains("Read 3 files (3 successful, 0 failed)"));
            assert!(result.contains("file1.txt"));
            assert!(result.contains("This is file 1"));
            assert!(result.contains("file2.txt"));
            assert!(result.contains("This is file 2"));
            assert!(result.contains("empty.txt"));
            assert!(result.contains("(empty file)"));
            // Check line numbers
            assert!(result.contains("     1\tThis is file 1"));
            assert!(result.contains("     2\tIt has multiple lines"));
        }
        _ => panic!("Expected ToolResult message"),
    }
}

#[tokio::test]
async fn test_read_many_files_nonexistent() {
    let (config, chat_ref, mut rx) = setup_test().await;
    
    // Create ReadManyFilesActor
    let read_many_actor = ReadManyFilesActor::new(config.clone());
    let (read_many_ref, _) = Actor::spawn(None, read_many_actor, config).await.unwrap();
    
    // Try to read nonexistent files
    let id = Uuid::new_v4();
    let params = json!({
        "paths": [
            "/nonexistent/file1.txt",
            "/nonexistent/file2.txt"
        ]
    });
    
    read_many_ref.send_message(ToolMessage::Execute {
        id,
        params,
        chat_ref,
    }).unwrap();
    
    // Wait for response
    let response = rx.recv().await.unwrap();
    match response {
        ChatMessage::ToolResult { id: res_id, result } => {
            assert_eq!(res_id, id);
            assert!(result.contains("Read 2 files (0 successful, 2 failed)"));
            assert!(result.contains("ERROR: File not found"));
        }
        _ => panic!("Expected ToolResult message"),
    }
}

#[tokio::test]
async fn test_read_many_files_mixed_results() {
    let (config, chat_ref, mut rx) = setup_test().await;
    let temp_dir = TempDir::new().unwrap();
    let mut file_paths = create_test_files(&temp_dir);
    
    // Add nonexistent file to the mix
    file_paths.push("/nonexistent/file.txt".to_string());
    
    // Create ReadManyFilesActor
    let read_many_actor = ReadManyFilesActor::new(config.clone());
    let (read_many_ref, _) = Actor::spawn(None, read_many_actor, config).await.unwrap();
    
    // Read files
    let id = Uuid::new_v4();
    let params = json!({
        "paths": file_paths
    });
    
    read_many_ref.send_message(ToolMessage::Execute {
        id,
        params,
        chat_ref,
    }).unwrap();
    
    // Wait for response
    let response = rx.recv().await.unwrap();
    match response {
        ChatMessage::ToolResult { id: res_id, result } => {
            assert_eq!(res_id, id);
            assert!(result.contains("Read 4 files (3 successful, 1 failed)"));
            assert!(result.contains("This is file 1"));
            assert!(result.contains("ERROR: File not found"));
        }
        _ => panic!("Expected ToolResult message"),
    }
}

#[tokio::test]
async fn test_read_many_files_max_lines() {
    let (config, chat_ref, mut rx) = setup_test().await;
    let temp_dir = TempDir::new().unwrap();
    
    // Create a file with many lines
    let file_path = temp_dir.path().join("many_lines.txt");
    let mut file = File::create(&file_path).unwrap();
    for i in 1..=10 {
        writeln!(file, "Line {}", i).unwrap();
    }
    
    // Create ReadManyFilesActor
    let read_many_actor = ReadManyFilesActor::new(config.clone());
    let (read_many_ref, _) = Actor::spawn(None, read_many_actor, config).await.unwrap();
    
    // Read with max_lines limit
    let id = Uuid::new_v4();
    let params = json!({
        "paths": [file_path.to_str().unwrap()],
        "max_lines_per_file": 5
    });
    
    read_many_ref.send_message(ToolMessage::Execute {
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
            assert!(result.contains("Line 5"));
            assert!(!result.contains("Line 6"));
            assert!(result.contains("(truncated 5 lines)"));
        }
        _ => panic!("Expected ToolResult message"),
    }
}

#[tokio::test]
async fn test_read_many_files_empty_paths() {
    let (config, chat_ref, mut rx) = setup_test().await;
    
    // Create ReadManyFilesActor
    let read_many_actor = ReadManyFilesActor::new(config.clone());
    let (read_many_ref, _) = Actor::spawn(None, read_many_actor, config).await.unwrap();
    
    // Send empty paths
    let id = Uuid::new_v4();
    let params = json!({
        "paths": []
    });
    
    read_many_ref.send_message(ToolMessage::Execute {
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
            assert!(result.contains("No file paths provided"));
        }
        _ => panic!("Expected ToolResult message"),
    }
}

#[tokio::test]
async fn test_read_many_files_invalid_parameters() {
    let (config, chat_ref, mut rx) = setup_test().await;
    
    // Create ReadManyFilesActor
    let read_many_actor = ReadManyFilesActor::new(config.clone());
    let (read_many_ref, _) = Actor::spawn(None, read_many_actor, config).await.unwrap();
    
    // Send invalid parameters
    let id = Uuid::new_v4();
    let params = json!({
        "invalid": "parameters"
    });
    
    read_many_ref.send_message(ToolMessage::Execute {
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
async fn test_read_many_files_directory() {
    let (config, chat_ref, mut rx) = setup_test().await;
    let temp_dir = TempDir::new().unwrap();
    
    // Create ReadManyFilesActor
    let read_many_actor = ReadManyFilesActor::new(config.clone());
    let (read_many_ref, _) = Actor::spawn(None, read_many_actor, config).await.unwrap();
    
    // Try to read a directory
    let id = Uuid::new_v4();
    let params = json!({
        "paths": [temp_dir.path().to_str().unwrap()]
    });
    
    read_many_ref.send_message(ToolMessage::Execute {
        id,
        params,
        chat_ref,
    }).unwrap();
    
    // Wait for response
    let response = rx.recv().await.unwrap();
    match response {
        ChatMessage::ToolResult { id: res_id, result } => {
            assert_eq!(res_id, id);
            assert!(result.contains("Read 1 files (0 successful, 1 failed)"));
            assert!(result.contains("ERROR: Not a file"));
        }
        _ => panic!("Expected ToolResult message"),
    }
}

#[tokio::test]
async fn test_read_many_files_default_max_lines() {
    let (config, chat_ref, mut rx) = setup_test().await;
    let temp_dir = TempDir::new().unwrap();
    
    // Create a file
    let file_path = temp_dir.path().join("test.txt");
    let mut file = File::create(&file_path).unwrap();
    writeln!(file, "Test content").unwrap();
    
    // Create ReadManyFilesActor
    let read_many_actor = ReadManyFilesActor::new(config.clone());
    let (read_many_ref, _) = Actor::spawn(None, read_many_actor, config).await.unwrap();
    
    // Read without specifying max_lines_per_file (should use default)
    let id = Uuid::new_v4();
    let params = json!({
        "paths": [file_path.to_str().unwrap()]
    });
    
    read_many_ref.send_message(ToolMessage::Execute {
        id,
        params,
        chat_ref,
    }).unwrap();
    
    // Wait for response
    let response = rx.recv().await.unwrap();
    match response {
        ChatMessage::ToolResult { id: res_id, result } => {
            assert_eq!(res_id, id);
            assert!(result.contains("Test content"));
            assert!(result.contains("Read 1 files (1 successful, 0 failed)"));
        }
        _ => panic!("Expected ToolResult message"),
    }
}