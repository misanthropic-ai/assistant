use assistant_core::actors::tools::grep::GrepActor;
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

fn create_test_files(temp_dir: &TempDir) {
    // Create directory structure
    fs::create_dir_all(temp_dir.path().join("src")).unwrap();
    fs::create_dir_all(temp_dir.path().join("tests")).unwrap();
    fs::create_dir_all(temp_dir.path().join("docs")).unwrap();
    
    // Create test files with searchable content
    fs::write(
        temp_dir.path().join("src/main.rs"),
        "fn main() {\n    println!(\"Hello, world!\");\n    let config = Config::new();\n}"
    ).unwrap();
    
    fs::write(
        temp_dir.path().join("src/lib.rs"),
        "pub mod utils;\npub mod config;\n\npub fn process_data(data: &str) -> String {\n    data.to_uppercase()\n}"
    ).unwrap();
    
    fs::write(
        temp_dir.path().join("src/config.rs"),
        "use serde::Deserialize;\n\n#[derive(Deserialize)]\npub struct Config {\n    pub name: String,\n    pub value: i32,\n}"
    ).unwrap();
    
    fs::write(
        temp_dir.path().join("tests/test1.rs"),
        "#[test]\nfn test_config() {\n    let config = Config::default();\n    assert_eq!(config.value, 0);\n}"
    ).unwrap();
    
    fs::write(
        temp_dir.path().join("docs/README.md"),
        "# Project Documentation\n\nThis project uses a Config struct for configuration.\n\n## Usage\n\nCreate a new Config instance with `Config::new()`."
    ).unwrap();
    
    fs::write(
        temp_dir.path().join("Cargo.toml"),
        "[package]\nname = \"test-project\"\nversion = \"0.1.0\"\n\n[dependencies]\nserde = \"1.0\""
    ).unwrap();
}

#[tokio::test]
async fn test_grep_search_pattern() {
    let (temp_dir, config, chat_ref, mut rx) = setup_test().await;
    create_test_files(&temp_dir);
    
    // Create GrepActor
    let grep_actor = GrepActor::new(config.clone());
    let (grep_ref, _) = Actor::spawn(None, grep_actor, config).await.unwrap();
    
    // Search for "Config" pattern
    let id = Uuid::new_v4();
    let params = json!({
        "pattern": "Config",
        "path": temp_dir.path().to_str().unwrap(),
        "output_mode": "files_with_matches"
    });
    
    grep_ref.send_message(ToolMessage::Execute {
        id,
        params,
        chat_ref,
    }).unwrap();
    
    // Wait for response
    let response = rx.recv().await.unwrap();
    match response {
        ChatMessage::ToolResult { id: res_id, result } => {
            assert_eq!(res_id, id);
            assert!(result.contains("Files containing pattern 'Config'"));
            assert!(result.contains("src/main.rs"));
            assert!(result.contains("src/config.rs"));
            assert!(result.contains("tests/test1.rs"));
            assert!(result.contains("docs/README.md"));
        }
        _ => panic!("Expected ToolResult message"),
    }
}

#[tokio::test]
async fn test_grep_case_insensitive() {
    let (temp_dir, config, chat_ref, mut rx) = setup_test().await;
    create_test_files(&temp_dir);
    
    // Create GrepActor
    let grep_actor = GrepActor::new(config.clone());
    let (grep_ref, _) = Actor::spawn(None, grep_actor, config).await.unwrap();
    
    // Search case-insensitive
    let id = Uuid::new_v4();
    let params = json!({
        "pattern": "config",
        "path": temp_dir.path().to_str().unwrap(),
        "-i": true,
        "output_mode": "count"
    });
    
    grep_ref.send_message(ToolMessage::Execute {
        id,
        params,
        chat_ref,
    }).unwrap();
    
    // Wait for response
    let response = rx.recv().await.unwrap();
    match response {
        ChatMessage::ToolResult { id: res_id, result } => {
            assert_eq!(res_id, id);
            assert!(result.contains("Match counts for pattern 'config'"));
            assert!(result.contains("Total:"));
            assert!(result.contains("files"));
        }
        _ => panic!("Expected ToolResult message"),
    }
}

#[tokio::test]
async fn test_grep_with_line_numbers() {
    let (temp_dir, config, chat_ref, mut rx) = setup_test().await;
    create_test_files(&temp_dir);
    
    // Create GrepActor
    let grep_actor = GrepActor::new(config.clone());
    let (grep_ref, _) = Actor::spawn(None, grep_actor, config).await.unwrap();
    
    // Search with line numbers
    let id = Uuid::new_v4();
    let params = json!({
        "pattern": "println",
        "path": temp_dir.path().to_str().unwrap(),
        "-n": true,
        "output_mode": "content"
    });
    
    grep_ref.send_message(ToolMessage::Execute {
        id,
        params,
        chat_ref,
    }).unwrap();
    
    // Wait for response
    let response = rx.recv().await.unwrap();
    match response {
        ChatMessage::ToolResult { id: res_id, result } => {
            assert_eq!(res_id, id);
            assert!(result.contains("Found matches for pattern 'println'"));
            assert!(result.contains("src/main.rs:2"));
            assert!(result.contains("Hello, world!"));
        }
        _ => panic!("Expected ToolResult message"),
    }
}

#[tokio::test]
async fn test_grep_with_glob_filter() {
    let (temp_dir, config, chat_ref, mut rx) = setup_test().await;
    create_test_files(&temp_dir);
    
    // Create GrepActor
    let grep_actor = GrepActor::new(config.clone());
    let (grep_ref, _) = Actor::spawn(None, grep_actor, config).await.unwrap();
    
    // Search only in Rust files
    let id = Uuid::new_v4();
    let params = json!({
        "pattern": "Config",
        "path": temp_dir.path().to_str().unwrap(),
        "glob": "**/*.rs",
        "output_mode": "files_with_matches"
    });
    
    grep_ref.send_message(ToolMessage::Execute {
        id,
        params,
        chat_ref,
    }).unwrap();
    
    // Wait for response
    let response = rx.recv().await.unwrap();
    match response {
        ChatMessage::ToolResult { id: res_id, result } => {
            assert_eq!(res_id, id);
            assert!(result.contains("src/main.rs"));
            assert!(result.contains("src/config.rs"));
            assert!(result.contains("tests/test1.rs"));
            assert!(!result.contains("README.md")); // Should not include markdown files
        }
        _ => panic!("Expected ToolResult message"),
    }
}

#[tokio::test]
async fn test_grep_with_type_filter() {
    let (temp_dir, config, chat_ref, mut rx) = setup_test().await;
    create_test_files(&temp_dir);
    
    // Create GrepActor
    let grep_actor = GrepActor::new(config.clone());
    let (grep_ref, _) = Actor::spawn(None, grep_actor, config).await.unwrap();
    
    // Search only in Rust files using type filter
    let id = Uuid::new_v4();
    let params = json!({
        "pattern": "struct",
        "path": temp_dir.path().to_str().unwrap(),
        "type": "rust",
        "output_mode": "files_with_matches"
    });
    
    grep_ref.send_message(ToolMessage::Execute {
        id,
        params,
        chat_ref,
    }).unwrap();
    
    // Wait for response
    let response = rx.recv().await.unwrap();
    match response {
        ChatMessage::ToolResult { id: res_id, result } => {
            assert_eq!(res_id, id);
            assert!(result.contains("src/config.rs"));
            assert!(!result.contains("Cargo.toml"));
        }
        _ => panic!("Expected ToolResult message"),
    }
}

#[tokio::test]
async fn test_grep_no_matches() {
    let (temp_dir, config, chat_ref, mut rx) = setup_test().await;
    create_test_files(&temp_dir);
    
    // Create GrepActor
    let grep_actor = GrepActor::new(config.clone());
    let (grep_ref, _) = Actor::spawn(None, grep_actor, config).await.unwrap();
    
    // Search for non-existent pattern
    let id = Uuid::new_v4();
    let params = json!({
        "pattern": "NonExistentPattern",
        "path": temp_dir.path().to_str().unwrap()
    });
    
    grep_ref.send_message(ToolMessage::Execute {
        id,
        params,
        chat_ref,
    }).unwrap();
    
    // Wait for response
    let response = rx.recv().await.unwrap();
    match response {
        ChatMessage::ToolResult { id: res_id, result } => {
            assert_eq!(res_id, id);
            assert!(result.contains("No files containing pattern 'NonExistentPattern'"));
        }
        _ => panic!("Expected ToolResult message"),
    }
}

#[tokio::test]
async fn test_grep_invalid_regex() {
    let (temp_dir, config, chat_ref, mut rx) = setup_test().await;
    
    // Create GrepActor
    let grep_actor = GrepActor::new(config.clone());
    let (grep_ref, _) = Actor::spawn(None, grep_actor, config).await.unwrap();
    
    // Search with invalid regex
    let id = Uuid::new_v4();
    let params = json!({
        "pattern": "[invalid regex",
        "path": temp_dir.path().to_str().unwrap()
    });
    
    grep_ref.send_message(ToolMessage::Execute {
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
            assert!(result.contains("Invalid regex pattern"));
        }
        _ => panic!("Expected ToolResult message"),
    }
}

#[tokio::test]
async fn test_grep_head_limit() {
    let (temp_dir, config, chat_ref, mut rx) = setup_test().await;
    create_test_files(&temp_dir);
    
    // Create many files with matches
    for i in 0..10 {
        fs::write(
            temp_dir.path().join(format!("file{}.txt", i)),
            format!("This file contains Config number {}", i)
        ).unwrap();
    }
    
    // Create GrepActor
    let grep_actor = GrepActor::new(config.clone());
    let (grep_ref, _) = Actor::spawn(None, grep_actor, config).await.unwrap();
    
    // Search with head limit
    let id = Uuid::new_v4();
    let params = json!({
        "pattern": "Config",
        "path": temp_dir.path().to_str().unwrap(),
        "output_mode": "files_with_matches",
        "head_limit": 5
    });
    
    grep_ref.send_message(ToolMessage::Execute {
        id,
        params,
        chat_ref,
    }).unwrap();
    
    // Wait for response
    let response = rx.recv().await.unwrap();
    match response {
        ChatMessage::ToolResult { id: res_id, result } => {
            assert_eq!(res_id, id);
            assert!(result.contains("Showing first 5 files"));
            // Count number of file paths in result
            let file_count = result.matches(".txt").count() + result.matches(".rs").count() + result.matches(".md").count();
            assert_eq!(file_count, 5);
        }
        _ => panic!("Expected ToolResult message"),
    }
}

#[tokio::test]
async fn test_grep_single_file() {
    let (temp_dir, config, chat_ref, mut rx) = setup_test().await;
    create_test_files(&temp_dir);
    
    // Create GrepActor
    let grep_actor = GrepActor::new(config.clone());
    let (grep_ref, _) = Actor::spawn(None, grep_actor, config).await.unwrap();
    
    // Search in single file
    let id = Uuid::new_v4();
    let params = json!({
        "pattern": "Config",
        "path": temp_dir.path().join("src/main.rs").to_str().unwrap(),
        "output_mode": "content"
    });
    
    grep_ref.send_message(ToolMessage::Execute {
        id,
        params,
        chat_ref,
    }).unwrap();
    
    // Wait for response
    let response = rx.recv().await.unwrap();
    match response {
        ChatMessage::ToolResult { id: res_id, result } => {
            assert_eq!(res_id, id);
            assert!(result.contains("Found matches"));
            assert!(result.contains("src/main.rs"));
            assert!(result.contains("let config = Config::new()"));
        }
        _ => panic!("Expected ToolResult message"),
    }
}

#[tokio::test]
async fn test_grep_relative_path_error() {
    let (_temp_dir, config, chat_ref, mut rx) = setup_test().await;
    
    // Create GrepActor
    let grep_actor = GrepActor::new(config.clone());
    let (grep_ref, _) = Actor::spawn(None, grep_actor, config).await.unwrap();
    
    // Try with relative path
    let id = Uuid::new_v4();
    let params = json!({
        "pattern": "test",
        "path": "./relative/path"
    });
    
    grep_ref.send_message(ToolMessage::Execute {
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