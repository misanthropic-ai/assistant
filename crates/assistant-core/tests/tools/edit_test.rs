use assistant_core::actors::tools::edit::EditActor;
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
async fn test_edit_file_single_replacement() {
    let (temp_dir, config, chat_ref, mut rx) = setup_test().await;
    
    // Create a test file
    let file_path = temp_dir.path().join("test.txt");
    let original_content = "Hello, world!\nThis is a test file.\nGoodbye, world!";
    fs::write(&file_path, original_content).unwrap();
    
    // Create EditActor
    let edit_actor = EditActor::new(config.clone());
    let (edit_ref, _) = Actor::spawn(None, edit_actor, config).await.unwrap();
    
    // Edit the file
    let id = Uuid::new_v4();
    let params = json!({
        "file_path": file_path.to_str().unwrap(),
        "old_string": "world",
        "new_string": "universe",
        "expected_replacements": 1
    });
    
    edit_ref.send_message(ToolMessage::Execute {
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
            assert!(result.contains("Expected 1 replacements but found 2 occurrences"));
        }
        _ => panic!("Expected ToolResult message"),
    }
    
    // Verify file was not modified
    let content = fs::read_to_string(&file_path).unwrap();
    assert_eq!(content, original_content);
}

#[tokio::test]
async fn test_edit_file_multiple_replacements() {
    let (temp_dir, config, chat_ref, mut rx) = setup_test().await;
    
    // Create a test file
    let file_path = temp_dir.path().join("test.txt");
    let original_content = "Hello, world!\nThis is a test file.\nGoodbye, world!";
    fs::write(&file_path, original_content).unwrap();
    
    // Create EditActor
    let edit_actor = EditActor::new(config.clone());
    let (edit_ref, _) = Actor::spawn(None, edit_actor, config).await.unwrap();
    
    // Edit the file with correct expected_replacements
    let id = Uuid::new_v4();
    let params = json!({
        "file_path": file_path.to_str().unwrap(),
        "old_string": "world",
        "new_string": "universe",
        "expected_replacements": 2
    });
    
    edit_ref.send_message(ToolMessage::Execute {
        id,
        params,
        chat_ref,
    }).unwrap();
    
    // Wait for response
    let response = rx.recv().await.unwrap();
    match response {
        ChatMessage::ToolResult { id: res_id, result } => {
            assert_eq!(res_id, id);
            assert!(result.contains("Successfully edited file"));
            assert!(result.contains("Replacements made: 2"));
        }
        _ => panic!("Expected ToolResult message"),
    }
    
    // Verify file was modified correctly
    let content = fs::read_to_string(&file_path).unwrap();
    assert_eq!(content, "Hello, universe!\nThis is a test file.\nGoodbye, universe!");
}

#[tokio::test]
async fn test_create_new_file_with_edit() {
    let (temp_dir, config, chat_ref, mut rx) = setup_test().await;
    
    // Create EditActor
    let edit_actor = EditActor::new(config.clone());
    let (edit_ref, _) = Actor::spawn(None, edit_actor, config).await.unwrap();
    
    // Create a new file using empty old_string
    let id = Uuid::new_v4();
    let file_path = temp_dir.path().join("new_file.txt");
    let new_content = "This is a new file\nCreated with the edit tool";
    let params = json!({
        "file_path": file_path.to_str().unwrap(),
        "old_string": "",
        "new_string": new_content
    });
    
    edit_ref.send_message(ToolMessage::Execute {
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
            assert!(result.contains("Lines: 2"));
        }
        _ => panic!("Expected ToolResult message"),
    }
    
    // Verify file was created
    assert!(file_path.exists());
    let content = fs::read_to_string(&file_path).unwrap();
    assert_eq!(content, new_content);
}

#[tokio::test]
async fn test_edit_with_multiline_strings() {
    let (temp_dir, config, chat_ref, mut rx) = setup_test().await;
    
    // Create a test file
    let file_path = temp_dir.path().join("multiline.txt");
    let original_content = "function hello() {\n    console.log('Hello, world!');\n    return true;\n}";
    fs::write(&file_path, original_content).unwrap();
    
    // Create EditActor
    let edit_actor = EditActor::new(config.clone());
    let (edit_ref, _) = Actor::spawn(None, edit_actor, config).await.unwrap();
    
    // Edit with multiline strings
    let id = Uuid::new_v4();
    let params = json!({
        "file_path": file_path.to_str().unwrap(),
        "old_string": "function hello() {\n    console.log('Hello, world!');\n    return true;\n}",
        "new_string": "function hello() {\n    console.log('Hello, universe!');\n    console.log('How are you?');\n    return true;\n}"
    });
    
    edit_ref.send_message(ToolMessage::Execute {
        id,
        params,
        chat_ref,
    }).unwrap();
    
    // Wait for response
    let response = rx.recv().await.unwrap();
    match response {
        ChatMessage::ToolResult { id: res_id, result } => {
            assert_eq!(res_id, id);
            assert!(result.contains("Successfully edited file"));
            assert!(result.contains("Lines: 4 → 5 (+1)"));
        }
        _ => panic!("Expected ToolResult message"),
    }
    
    // Verify file was modified correctly
    let content = fs::read_to_string(&file_path).unwrap();
    assert!(content.contains("Hello, universe!"));
    assert!(content.contains("How are you?"));
}

#[tokio::test]
async fn test_edit_no_matches_found() {
    let (temp_dir, config, chat_ref, mut rx) = setup_test().await;
    
    // Create a test file
    let file_path = temp_dir.path().join("test.txt");
    let original_content = "Hello, world!";
    fs::write(&file_path, original_content).unwrap();
    
    // Create EditActor
    let edit_actor = EditActor::new(config.clone());
    let (edit_ref, _) = Actor::spawn(None, edit_actor, config).await.unwrap();
    
    // Try to edit with non-existent string
    let id = Uuid::new_v4();
    let params = json!({
        "file_path": file_path.to_str().unwrap(),
        "old_string": "universe",
        "new_string": "galaxy"
    });
    
    edit_ref.send_message(ToolMessage::Execute {
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
            assert!(result.contains("No matches found"));
        }
        _ => panic!("Expected ToolResult message"),
    }
    
    // Verify file was not modified
    let content = fs::read_to_string(&file_path).unwrap();
    assert_eq!(content, original_content);
}

#[tokio::test]
async fn test_edit_file_not_found() {
    let (temp_dir, config, chat_ref, mut rx) = setup_test().await;
    
    // Create EditActor
    let edit_actor = EditActor::new(config.clone());
    let (edit_ref, _) = Actor::spawn(None, edit_actor, config).await.unwrap();
    
    // Try to edit non-existent file
    let id = Uuid::new_v4();
    let file_path = temp_dir.path().join("non_existent.txt");
    let params = json!({
        "file_path": file_path.to_str().unwrap(),
        "old_string": "something",
        "new_string": "else"
    });
    
    edit_ref.send_message(ToolMessage::Execute {
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
            assert!(result.contains("File not found"));
        }
        _ => panic!("Expected ToolResult message"),
    }
}

#[tokio::test]
async fn test_relative_path_error() {
    let (_temp_dir, config, chat_ref, mut rx) = setup_test().await;
    
    // Create EditActor
    let edit_actor = EditActor::new(config.clone());
    let (edit_ref, _) = Actor::spawn(None, edit_actor, config).await.unwrap();
    
    // Try to edit with relative path
    let id = Uuid::new_v4();
    let params = json!({
        "file_path": "./relative/path.txt",
        "old_string": "something",
        "new_string": "else"
    });
    
    edit_ref.send_message(ToolMessage::Execute {
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
            assert!(!result.contains("Error") || result.contains("Cannot access path"));
        }
        _ => panic!("Expected ToolResult message"),
    }
}

#[tokio::test]
async fn test_invalid_parameters() {
    let (_temp_dir, config, chat_ref, mut rx) = setup_test().await;
    
    // Create EditActor
    let edit_actor = EditActor::new(config.clone());
    let (edit_ref, _) = Actor::spawn(None, edit_actor, config).await.unwrap();
    
    // Send invalid parameters (missing new_string)
    let id = Uuid::new_v4();
    let params = json!({
        "file_path": "/tmp/test.txt",
        "old_string": "something"
    });
    
    edit_ref.send_message(ToolMessage::Execute {
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