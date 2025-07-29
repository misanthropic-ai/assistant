use assistant_core::actors::tools::{BashActor, ToolMessage};
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

#[tokio::test]
async fn test_bash_simple_command() {
    let (_temp_dir, config, chat_ref, mut rx) = setup_test().await;
    
    // Create BashActor
    let bash_actor = BashActor::new(config.clone());
    let (bash_ref, _) = Actor::spawn(None, bash_actor, config).await.unwrap();
    
    // Execute simple echo command
    let id = Uuid::new_v4();
    let params = json!({
        "command": "echo 'Hello, World!'",
        "description": "Test echo command"
    });
    
    bash_ref.send_message(ToolMessage::Execute {
        id,
        params,
        chat_ref,
    }).unwrap();
    
    // Wait for response
    let response = rx.recv().await.unwrap();
    match response {
        ChatMessage::ToolResult { id: res_id, result } => {
            assert_eq!(res_id, id);
            assert!(result.contains("Hello, World!"));
        }
        _ => panic!("Expected ToolResult message"),
    }
}

#[tokio::test]
async fn test_bash_with_timeout() {
    let (_temp_dir, config, chat_ref, mut rx) = setup_test().await;
    
    // Create BashActor
    let bash_actor = BashActor::new(config.clone());
    let (bash_ref, _) = Actor::spawn(None, bash_actor, config).await.unwrap();
    
    // Execute command that times out
    let id = Uuid::new_v4();
    let params = json!({
        "command": "sleep 5",
        "timeout": 1000  // 1 second timeout
    });
    
    bash_ref.send_message(ToolMessage::Execute {
        id,
        params,
        chat_ref,
    }).unwrap();
    
    // Wait for response
    let response = rx.recv().await.unwrap();
    match response {
        ChatMessage::ToolResult { id: res_id, result } => {
            assert_eq!(res_id, id);
            assert!(result.contains("timed out"));
            assert!(result.contains("1000ms"));
        }
        _ => panic!("Expected ToolResult message"),
    }
}

#[tokio::test]
async fn test_bash_cd_command() {
    let (temp_dir, config, chat_ref, mut rx) = setup_test().await;
    
    // Create a subdirectory
    let subdir = temp_dir.path().join("subdir");
    fs::create_dir(&subdir).unwrap();
    
    // Create BashActor
    let bash_actor = BashActor::new(config.clone());
    let (bash_ref, _) = Actor::spawn(None, bash_actor, config).await.unwrap();
    
    // Change to temp directory first
    let id1 = Uuid::new_v4();
    let params1 = json!({
        "command": format!("cd {}", temp_dir.path().display())
    });
    
    bash_ref.send_message(ToolMessage::Execute {
        id: id1,
        params: params1,
        chat_ref: chat_ref.clone(),
    }).unwrap();
    
    let response1 = rx.recv().await.unwrap();
    match response1 {
        ChatMessage::ToolResult { result, .. } => {
            assert!(result.contains("Changed directory to"));
        }
        _ => panic!("Expected ToolResult message"),
    }
    
    // Now cd to subdirectory
    let id2 = Uuid::new_v4();
    let params2 = json!({
        "command": "cd subdir"
    });
    
    bash_ref.send_message(ToolMessage::Execute {
        id: id2,
        params: params2,
        chat_ref: chat_ref.clone(),
    }).unwrap();
    
    let response2 = rx.recv().await.unwrap();
    match response2 {
        ChatMessage::ToolResult { result, .. } => {
            assert!(result.contains("Changed directory to"));
            assert!(result.contains("subdir"));
        }
        _ => panic!("Expected ToolResult message"),
    }
    
    // Verify working directory persists
    let id3 = Uuid::new_v4();
    let params3 = json!({
        "command": "pwd"
    });
    
    bash_ref.send_message(ToolMessage::Execute {
        id: id3,
        params: params3,
        chat_ref,
    }).unwrap();
    
    let response3 = rx.recv().await.unwrap();
    match response3 {
        ChatMessage::ToolResult { result, .. } => {
            assert!(result.contains("subdir"));
        }
        _ => panic!("Expected ToolResult message"),
    }
}

#[tokio::test]
async fn test_bash_environment_variables() {
    let (_temp_dir, config, chat_ref, mut rx) = setup_test().await;
    
    // Create BashActor
    let bash_actor = BashActor::new(config.clone());
    let (bash_ref, _) = Actor::spawn(None, bash_actor, config).await.unwrap();
    
    // Check environment variables
    let id = Uuid::new_v4();
    let params = json!({
        "command": "echo $NO_COLOR $TERM"
    });
    
    bash_ref.send_message(ToolMessage::Execute {
        id,
        params,
        chat_ref,
    }).unwrap();
    
    // Wait for response
    let response = rx.recv().await.unwrap();
    match response {
        ChatMessage::ToolResult { id: res_id, result } => {
            assert_eq!(res_id, id);
            assert!(result.contains("1 dumb"));
        }
        _ => panic!("Expected ToolResult message"),
    }
}

#[tokio::test]
async fn test_bash_command_failure() {
    let (_temp_dir, config, chat_ref, mut rx) = setup_test().await;
    
    // Create BashActor
    let bash_actor = BashActor::new(config.clone());
    let (bash_ref, _) = Actor::spawn(None, bash_actor, config).await.unwrap();
    
    // Execute failing command
    let id = Uuid::new_v4();
    let params = json!({
        "command": "exit 42"
    });
    
    bash_ref.send_message(ToolMessage::Execute {
        id,
        params,
        chat_ref,
    }).unwrap();
    
    // Wait for response
    let response = rx.recv().await.unwrap();
    match response {
        ChatMessage::ToolResult { id: res_id, result } => {
            assert_eq!(res_id, id);
            assert!(result.contains("exited with code: 42"));
        }
        _ => panic!("Expected ToolResult message"),
    }
}

#[tokio::test]
async fn test_bash_stderr_capture() {
    let (_temp_dir, config, chat_ref, mut rx) = setup_test().await;
    
    // Create BashActor
    let bash_actor = BashActor::new(config.clone());
    let (bash_ref, _) = Actor::spawn(None, bash_actor, config).await.unwrap();
    
    // Execute command that writes to stderr
    let id = Uuid::new_v4();
    let params = json!({
        "command": "echo 'Error message' >&2"
    });
    
    bash_ref.send_message(ToolMessage::Execute {
        id,
        params,
        chat_ref,
    }).unwrap();
    
    // Wait for response
    let response = rx.recv().await.unwrap();
    match response {
        ChatMessage::ToolResult { id: res_id, result } => {
            assert_eq!(res_id, id);
            assert!(result.contains("Error message"));
        }
        _ => panic!("Expected ToolResult message"),
    }
}

#[tokio::test]
async fn test_bash_multiline_output() {
    let (_temp_dir, config, chat_ref, mut rx) = setup_test().await;
    
    // Create BashActor
    let bash_actor = BashActor::new(config.clone());
    let (bash_ref, _) = Actor::spawn(None, bash_actor, config).await.unwrap();
    
    // Execute command with multiline output
    let id = Uuid::new_v4();
    let params = json!({
        "command": "echo -e 'Line 1\\nLine 2\\nLine 3'"
    });
    
    bash_ref.send_message(ToolMessage::Execute {
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
async fn test_bash_timeout_validation() {
    let (_temp_dir, config, chat_ref, mut rx) = setup_test().await;
    
    // Create BashActor
    let bash_actor = BashActor::new(config.clone());
    let (bash_ref, _) = Actor::spawn(None, bash_actor, config).await.unwrap();
    
    // Try to exceed maximum timeout
    let id = Uuid::new_v4();
    let params = json!({
        "command": "echo test",
        "timeout": 700000  // 11+ minutes
    });
    
    bash_ref.send_message(ToolMessage::Execute {
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
            assert!(result.contains("Timeout exceeds maximum"));
        }
        _ => panic!("Expected ToolResult message"),
    }
}

#[tokio::test]
async fn test_bash_invalid_cd() {
    let (_temp_dir, config, chat_ref, mut rx) = setup_test().await;
    
    // Create BashActor
    let bash_actor = BashActor::new(config.clone());
    let (bash_ref, _) = Actor::spawn(None, bash_actor, config).await.unwrap();
    
    // Try to cd to non-existent directory
    let id = Uuid::new_v4();
    let params = json!({
        "command": "cd /non/existent/directory"
    });
    
    bash_ref.send_message(ToolMessage::Execute {
        id,
        params,
        chat_ref,
    }).unwrap();
    
    // Wait for response
    let response = rx.recv().await.unwrap();
    match response {
        ChatMessage::ToolResult { id: res_id, result } => {
            assert_eq!(res_id, id);
            assert!(result.contains("Error changing directory"));
        }
        _ => panic!("Expected ToolResult message"),
    }
}

#[tokio::test]
async fn test_bash_home_directory() {
    let (_temp_dir, config, chat_ref, mut rx) = setup_test().await;
    
    // Create BashActor
    let bash_actor = BashActor::new(config.clone());
    let (bash_ref, _) = Actor::spawn(None, bash_actor, config).await.unwrap();
    
    // cd to home directory
    let id = Uuid::new_v4();
    let params = json!({
        "command": "cd ~"
    });
    
    bash_ref.send_message(ToolMessage::Execute {
        id,
        params,
        chat_ref,
    }).unwrap();
    
    // Wait for response
    let response = rx.recv().await.unwrap();
    match response {
        ChatMessage::ToolResult { id: res_id, result } => {
            assert_eq!(res_id, id);
            assert!(result.contains("Changed directory to"));
            // Should contain home directory path
            let home = dirs::home_dir().unwrap();
            assert!(result.contains(&home.to_string_lossy().to_string()));
        }
        _ => panic!("Expected ToolResult message"),
    }
}