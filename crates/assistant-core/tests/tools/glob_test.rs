use assistant_core::actors::tools::{GlobActor, ToolMessage};
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
    let config = Config::default();
    
    let (tx, rx) = mpsc::unbounded_channel();
    let mock_chat = MockChatActor { sender: tx.clone() };
    let (chat_ref, _) = Actor::spawn(None, mock_chat, tx).await.unwrap();
    
    (temp_dir, config, chat_ref, rx)
}

fn create_test_files(temp_dir: &TempDir) {
    // Create a directory structure
    fs::create_dir_all(temp_dir.path().join("src")).unwrap();
    fs::create_dir_all(temp_dir.path().join("tests")).unwrap();
    fs::create_dir_all(temp_dir.path().join("docs")).unwrap();
    
    // Create some test files
    fs::write(temp_dir.path().join("README.md"), "# Test Project").unwrap();
    fs::write(temp_dir.path().join("Cargo.toml"), "[package]\nname = \"test\"").unwrap();
    fs::write(temp_dir.path().join("src/main.rs"), "fn main() {}").unwrap();
    fs::write(temp_dir.path().join("src/lib.rs"), "pub mod utils;").unwrap();
    fs::write(temp_dir.path().join("src/utils.rs"), "pub fn helper() {}").unwrap();
    fs::write(temp_dir.path().join("tests/test1.rs"), "#[test]\nfn test1() {}").unwrap();
    fs::write(temp_dir.path().join("tests/test2.rs"), "#[test]\nfn test2() {}").unwrap();
    fs::write(temp_dir.path().join("docs/guide.md"), "# User Guide").unwrap();
    fs::write(temp_dir.path().join(".gitignore"), "target/\n*.log").unwrap();
}

#[tokio::test]
async fn test_glob_find_rust_files() {
    let (temp_dir, config, chat_ref, mut rx) = setup_test().await;
    create_test_files(&temp_dir);
    
    // Create GlobActor
    let glob_actor = GlobActor::new(config.clone());
    let (glob_ref, _) = Actor::spawn(None, glob_actor, config).await.unwrap();
    
    // Search for Rust files
    let id = Uuid::new_v4();
    let params = json!({
        "pattern": "**/*.rs",
        "path": temp_dir.path().to_str().unwrap()
    });
    
    glob_ref.send_message(ToolMessage::Execute {
        id,
        params,
        chat_ref,
    }).unwrap();
    
    // Wait for response
    let response = rx.recv().await.unwrap();
    match response {
        ChatMessage::ToolResult { id: res_id, result } => {
            assert_eq!(res_id, id);
            assert!(result.contains("Found 5 files"));
            assert!(result.contains("src/main.rs"));
            assert!(result.contains("src/lib.rs"));
            assert!(result.contains("src/utils.rs"));
            assert!(result.contains("tests/test1.rs"));
            assert!(result.contains("tests/test2.rs"));
        }
        _ => panic!("Expected ToolResult message"),
    }
}

#[tokio::test]
async fn test_glob_find_markdown_files() {
    let (temp_dir, config, chat_ref, mut rx) = setup_test().await;
    create_test_files(&temp_dir);
    
    // Create GlobActor
    let glob_actor = GlobActor::new(config.clone());
    let (glob_ref, _) = Actor::spawn(None, glob_actor, config).await.unwrap();
    
    // Search for Markdown files
    let id = Uuid::new_v4();
    let params = json!({
        "pattern": "**/*.md",
        "path": temp_dir.path().to_str().unwrap()
    });
    
    glob_ref.send_message(ToolMessage::Execute {
        id,
        params,
        chat_ref,
    }).unwrap();
    
    // Wait for response
    let response = rx.recv().await.unwrap();
    match response {
        ChatMessage::ToolResult { id: res_id, result } => {
            assert_eq!(res_id, id);
            assert!(result.contains("Found 2 files"));
            assert!(result.contains("README.md"));
            assert!(result.contains("docs/guide.md"));
        }
        _ => panic!("Expected ToolResult message"),
    }
}

#[tokio::test]
async fn test_glob_specific_directory() {
    let (temp_dir, config, chat_ref, mut rx) = setup_test().await;
    create_test_files(&temp_dir);
    
    // Create GlobActor
    let glob_actor = GlobActor::new(config.clone());
    let (glob_ref, _) = Actor::spawn(None, glob_actor, config).await.unwrap();
    
    // Search for files in src directory only
    let id = Uuid::new_v4();
    let params = json!({
        "pattern": "*.rs",
        "path": temp_dir.path().join("src").to_str().unwrap()
    });
    
    glob_ref.send_message(ToolMessage::Execute {
        id,
        params,
        chat_ref,
    }).unwrap();
    
    // Wait for response
    let response = rx.recv().await.unwrap();
    match response {
        ChatMessage::ToolResult { id: res_id, result } => {
            assert_eq!(res_id, id);
            assert!(result.contains("Found 3 files"));
            assert!(result.contains("main.rs"));
            assert!(result.contains("lib.rs"));
            assert!(result.contains("utils.rs"));
            assert!(!result.contains("test1.rs")); // Should not include test files
        }
        _ => panic!("Expected ToolResult message"),
    }
}

#[tokio::test]
async fn test_glob_no_matches() {
    let (temp_dir, config, chat_ref, mut rx) = setup_test().await;
    create_test_files(&temp_dir);
    
    // Create GlobActor
    let glob_actor = GlobActor::new(config.clone());
    let (glob_ref, _) = Actor::spawn(None, glob_actor, config).await.unwrap();
    
    // Search for Python files (none exist)
    let id = Uuid::new_v4();
    let params = json!({
        "pattern": "**/*.py",
        "path": temp_dir.path().to_str().unwrap()
    });
    
    glob_ref.send_message(ToolMessage::Execute {
        id,
        params,
        chat_ref,
    }).unwrap();
    
    // Wait for response
    let response = rx.recv().await.unwrap();
    match response {
        ChatMessage::ToolResult { id: res_id, result } => {
            assert_eq!(res_id, id);
            assert!(result.contains("No files found"));
            assert!(result.contains("*.py"));
        }
        _ => panic!("Expected ToolResult message"),
    }
}

#[tokio::test]
async fn test_glob_case_sensitive() {
    let (temp_dir, config, chat_ref, mut rx) = setup_test().await;
    
    // Create files with different cases
    fs::write(temp_dir.path().join("Test.txt"), "test").unwrap();
    fs::write(temp_dir.path().join("test.TXT"), "test").unwrap();
    
    // Create GlobActor
    let glob_actor = GlobActor::new(config.clone());
    let (glob_ref, _) = Actor::spawn(None, glob_actor, config).await.unwrap();
    
    // Search case-sensitive
    let id = Uuid::new_v4();
    let params = json!({
        "pattern": "*.txt",
        "path": temp_dir.path().to_str().unwrap(),
        "case_sensitive": true
    });
    
    glob_ref.send_message(ToolMessage::Execute {
        id,
        params,
        chat_ref,
    }).unwrap();
    
    // Wait for response
    let response = rx.recv().await.unwrap();
    match response {
        ChatMessage::ToolResult { id: res_id, result } => {
            assert_eq!(res_id, id);
            assert!(result.contains("Found 1 file"));
            assert!(result.contains("Test.txt"));
            assert!(!result.contains("test.TXT"));
        }
        _ => panic!("Expected ToolResult message"),
    }
}

#[tokio::test]
async fn test_glob_invalid_path() {
    let (_temp_dir, config, chat_ref, mut rx) = setup_test().await;
    
    // Create GlobActor
    let glob_actor = GlobActor::new(config.clone());
    let (glob_ref, _) = Actor::spawn(None, glob_actor, config).await.unwrap();
    
    // Search with non-existent path
    let id = Uuid::new_v4();
    let params = json!({
        "pattern": "*.rs",
        "path": "/non/existent/path"
    });
    
    glob_ref.send_message(ToolMessage::Execute {
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
            assert!(result.contains("Path does not exist"));
        }
        _ => panic!("Expected ToolResult message"),
    }
}

#[tokio::test]
async fn test_glob_relative_path_error() {
    let (_temp_dir, config, chat_ref, mut rx) = setup_test().await;
    
    // Create GlobActor
    let glob_actor = GlobActor::new(config.clone());
    let (glob_ref, _) = Actor::spawn(None, glob_actor, config).await.unwrap();
    
    // Try with relative path
    let id = Uuid::new_v4();
    let params = json!({
        "pattern": "*.rs",
        "path": "./relative/path"
    });
    
    glob_ref.send_message(ToolMessage::Execute {
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
            assert!(result.contains("Path must be absolute"));
        }
        _ => panic!("Expected ToolResult message"),
    }
}

#[tokio::test]
async fn test_glob_invalid_parameters() {
    let (_temp_dir, config, chat_ref, mut rx) = setup_test().await;
    
    // Create GlobActor
    let glob_actor = GlobActor::new(config.clone());
    let (glob_ref, _) = Actor::spawn(None, glob_actor, config).await.unwrap();
    
    // Send invalid parameters (missing pattern)
    let id = Uuid::new_v4();
    let params = json!({
        "path": "/tmp"
    });
    
    glob_ref.send_message(ToolMessage::Execute {
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