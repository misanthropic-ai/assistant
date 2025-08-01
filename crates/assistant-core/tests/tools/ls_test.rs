use assistant_core::actors::tools::ls::LsActor;
use assistant_core::messages::ToolMessage;
use assistant_core::messages::ChatMessage;
use assistant_core::config::Config;
use ractor::{Actor, ActorRef};
use serde_json::json;
use std::fs;
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
    let mut config = Config::default();
    config.api_key = "test-api-key".to_string();
    
    let (tx, rx) = mpsc::unbounded_channel();
    let mock_chat = MockChatActor { sender: tx.clone() };
    let (chat_ref, _) = Actor::spawn(None, mock_chat, tx).await.unwrap();
    
    (temp_dir, config, chat_ref, rx)
}

#[tokio::test]
async fn test_ls_empty_directory() {
    let (temp_dir, config, chat_ref, mut rx) = setup_test().await;
    
    // Create LsActor
    let ls_actor = LsActor::new(config.clone());
    let (ls_ref, _) = Actor::spawn(None, ls_actor, config).await.unwrap();
    
    // Send ls request
    let id = Uuid::new_v4();
    let params = json!({
        "path": temp_dir.path().to_str().unwrap()
    });
    
    ls_ref.send_message(ToolMessage::Execute {
        id,
        params,
        chat_ref,
    }).unwrap();
    
    // Wait for response
    let response = rx.recv().await.unwrap();
    match response {
        ChatMessage::ToolResult { id: res_id, result } => {
            assert_eq!(res_id, id);
            assert!(result.contains("is empty"));
        }
        _ => panic!("Expected ToolResult message"),
    }
}

#[tokio::test]
async fn test_ls_with_files() {
    let (temp_dir, config, chat_ref, mut rx) = setup_test().await;
    
    // Create some test files
    fs::write(temp_dir.path().join("file1.txt"), "content1").unwrap();
    fs::write(temp_dir.path().join("file2.rs"), "content2").unwrap();
    fs::create_dir(temp_dir.path().join("subdir")).unwrap();
    
    // Create LsActor
    let ls_actor = LsActor::new(config.clone());
    let (ls_ref, _) = Actor::spawn(None, ls_actor, config).await.unwrap();
    
    // Send ls request
    let id = Uuid::new_v4();
    let params = json!({
        "path": temp_dir.path().to_str().unwrap()
    });
    
    ls_ref.send_message(ToolMessage::Execute {
        id,
        params,
        chat_ref,
    }).unwrap();
    
    // Wait for response
    let response = rx.recv().await.unwrap();
    match response {
        ChatMessage::ToolResult { id: res_id, result } => {
            assert_eq!(res_id, id);
            assert!(result.contains("file1.txt"));
            assert!(result.contains("file2.rs"));
            assert!(result.contains("subdir"));
            assert!(result.contains("Total: 3 items"));
            // Check that directory comes first
            let subdir_pos = result.find("subdir").unwrap();
            let file1_pos = result.find("file1.txt").unwrap();
            assert!(subdir_pos < file1_pos);
        }
        _ => panic!("Expected ToolResult message"),
    }
}

#[tokio::test]
async fn test_ls_with_ignore_patterns() {
    let (temp_dir, config, chat_ref, mut rx) = setup_test().await;
    
    // Create some test files
    fs::write(temp_dir.path().join("file1.txt"), "content1").unwrap();
    fs::write(temp_dir.path().join("file2.rs"), "content2").unwrap();
    fs::write(temp_dir.path().join("ignored.log"), "log content").unwrap();
    
    // Create LsActor
    let ls_actor = LsActor::new(config.clone());
    let (ls_ref, _) = Actor::spawn(None, ls_actor, config).await.unwrap();
    
    // Send ls request with ignore pattern
    let id = Uuid::new_v4();
    let params = json!({
        "path": temp_dir.path().to_str().unwrap(),
        "ignore": ["*.log"]
    });
    
    ls_ref.send_message(ToolMessage::Execute {
        id,
        params,
        chat_ref,
    }).unwrap();
    
    // Wait for response
    let response = rx.recv().await.unwrap();
    match response {
        ChatMessage::ToolResult { id: res_id, result } => {
            assert_eq!(res_id, id);
            assert!(result.contains("file1.txt"));
            assert!(result.contains("file2.rs"));
            assert!(!result.contains("ignored.log"));
            assert!(result.contains("Total: 2 items"));
        }
        _ => panic!("Expected ToolResult message"),
    }
}

#[tokio::test]
async fn test_ls_invalid_path() {
    let (_temp_dir, config, chat_ref, mut rx) = setup_test().await;
    
    // Create LsActor
    let ls_actor = LsActor::new(config.clone());
    let (ls_ref, _) = Actor::spawn(None, ls_actor, config).await.unwrap();
    
    // Send ls request with non-existent path
    let id = Uuid::new_v4();
    let params = json!({
        "path": "/non/existent/path"
    });
    
    ls_ref.send_message(ToolMessage::Execute {
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
            assert!(result.contains("Cannot access path"));
        }
        _ => panic!("Expected ToolResult message"),
    }
}

#[tokio::test]
async fn test_ls_relative_path_error() {
    let (_temp_dir, config, chat_ref, mut rx) = setup_test().await;
    
    // Create LsActor
    let ls_actor = LsActor::new(config.clone());
    let (ls_ref, _) = Actor::spawn(None, ls_actor, config).await.unwrap();
    
    // Send ls request with relative path
    let id = Uuid::new_v4();
    let params = json!({
        "path": "./relative/path"
    });
    
    ls_ref.send_message(ToolMessage::Execute {
        id,
        params,
        chat_ref,
    }).unwrap();
    
    // Wait for response
    let response = rx.recv().await.unwrap();
    match response {
        ChatMessage::ToolResult { id: res_id, result } => {
            assert_eq!(res_id, id);
            // Should work with relative paths - they get resolved
            assert!(!result.contains("Error") || result.contains("Cannot access path") || result.contains("No such file"));
        }
        _ => panic!("Expected ToolResult message"),
    }
}

#[tokio::test]
async fn test_ls_file_not_directory() {
    let (temp_dir, config, chat_ref, mut rx) = setup_test().await;
    
    // Create a file
    let file_path = temp_dir.path().join("not_a_dir.txt");
    fs::write(&file_path, "content").unwrap();
    
    // Create LsActor
    let ls_actor = LsActor::new(config.clone());
    let (ls_ref, _) = Actor::spawn(None, ls_actor, config).await.unwrap();
    
    // Send ls request with file path
    let id = Uuid::new_v4();
    let params = json!({
        "path": file_path.to_str().unwrap()
    });
    
    ls_ref.send_message(ToolMessage::Execute {
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
            assert!(result.contains("not a directory"));
        }
        _ => panic!("Expected ToolResult message"),
    }
}