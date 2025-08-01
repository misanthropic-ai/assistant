use assistant_core::{
    actors::tools::bash::BashActor,
    config::Config,
    messages::{ToolMessage, ChatMessage},
    ractor::{Actor, ActorRef},
};
use serde_json::json;
use tokio::sync::mpsc;
use uuid::Uuid;

struct TestSetup {
    bash_ref: ActorRef<ToolMessage>,
    chat_ref: ActorRef<ChatMessage>,
    rx: mpsc::UnboundedReceiver<ChatMessage>,
}

async fn setup_bash_test() -> TestSetup {
    let mut config = Config::default();
    config.api_key = "test-api-key".to_string();
    
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
    
    // Create bash actor
    let bash_actor = BashActor::new(config.clone());
    let (bash_ref, _) = Actor::spawn(
        None,
        bash_actor,
        config,
    )
    .await
    .expect("Failed to spawn bash actor");
    
    TestSetup {
        bash_ref,
        chat_ref,
        rx,
    }
}

#[tokio::test]
async fn test_bash_echo_command() {
    let mut setup = setup_bash_test().await;
    
    // Run echo command
    let cmd_id = Uuid::new_v4();
    let params = json!({
        "command": "echo 'Hello, World!'"
    });
    
    setup.bash_ref
        .send_message(ToolMessage::Execute {
            id: cmd_id,
            params,
            chat_ref: setup.chat_ref.clone(),
        })
        .expect("Failed to send command");
    
    let response = setup.rx.recv().await.expect("Failed to receive response");
    match response {
        ChatMessage::ToolResult { id, result } => {
            assert_eq!(id, cmd_id);
            assert!(result.contains("Hello, World!"));
        }
        _ => panic!("Expected ToolResult"),
    }
}

#[tokio::test]
async fn test_bash_pwd_command() {
    let mut setup = setup_bash_test().await;
    
    // Run pwd command
    let cmd_id = Uuid::new_v4();
    let params = json!({
        "command": "pwd"
    });
    
    setup.bash_ref
        .send_message(ToolMessage::Execute {
            id: cmd_id,
            params,
            chat_ref: setup.chat_ref.clone(),
        })
        .expect("Failed to send command");
    
    let response = setup.rx.recv().await.expect("Failed to receive response");
    match response {
        ChatMessage::ToolResult { id, result } => {
            assert_eq!(id, cmd_id);
            // Should contain a path
            assert!(result.contains("/"));
        }
        _ => panic!("Expected ToolResult"),
    }
}

#[tokio::test]
async fn test_bash_multiple_commands() {
    let mut setup = setup_bash_test().await;
    
    // Run multiple commands with &&
    let cmd_id = Uuid::new_v4();
    let params = json!({
        "command": "echo 'First' && echo 'Second' && echo 'Third'"
    });
    
    setup.bash_ref
        .send_message(ToolMessage::Execute {
            id: cmd_id,
            params,
            chat_ref: setup.chat_ref.clone(),
        })
        .expect("Failed to send command");
    
    let response = setup.rx.recv().await.expect("Failed to receive response");
    match response {
        ChatMessage::ToolResult { id, result } => {
            assert_eq!(id, cmd_id);
            assert!(result.contains("First"));
            assert!(result.contains("Second"));
            assert!(result.contains("Third"));
        }
        _ => panic!("Expected ToolResult"),
    }
}

#[tokio::test]
async fn test_bash_error_handling() {
    let mut setup = setup_bash_test().await;
    
    // Run command that should fail
    let cmd_id = Uuid::new_v4();
    let params = json!({
        "command": "ls /nonexistent/directory/that/should/not/exist"
    });
    
    setup.bash_ref
        .send_message(ToolMessage::Execute {
            id: cmd_id,
            params,
            chat_ref: setup.chat_ref.clone(),
        })
        .expect("Failed to send command");
    
    let response = setup.rx.recv().await.expect("Failed to receive response");
    match response {
        ChatMessage::ToolResult { id, result } => {
            assert_eq!(id, cmd_id);
            // Should contain error message
            assert!(result.contains("Error") || result.contains("No such file") || result.contains("not found"));
        }
        _ => panic!("Expected ToolResult"),
    }
}

#[tokio::test]
async fn test_bash_environment_variables() {
    let mut setup = setup_bash_test().await;
    
    // Set and use environment variable
    let cmd_id = Uuid::new_v4();
    let params = json!({
        "command": "TEST_VAR='test_value' && echo $TEST_VAR"
    });
    
    setup.bash_ref
        .send_message(ToolMessage::Execute {
            id: cmd_id,
            params,
            chat_ref: setup.chat_ref.clone(),
        })
        .expect("Failed to send command");
    
    let response = setup.rx.recv().await.expect("Failed to receive response");
    match response {
        ChatMessage::ToolResult { id, result } => {
            assert_eq!(id, cmd_id);
            assert!(result.contains("test_value"));
        }
        _ => panic!("Expected ToolResult"),
    }
}

#[tokio::test]
async fn test_bash_piping() {
    let mut setup = setup_bash_test().await;
    
    // Test piping commands
    let cmd_id = Uuid::new_v4();
    let params = json!({
        "command": "echo -e 'line1\\nline2\\nline3' | grep line2"
    });
    
    setup.bash_ref
        .send_message(ToolMessage::Execute {
            id: cmd_id,
            params,
            chat_ref: setup.chat_ref.clone(),
        })
        .expect("Failed to send command");
    
    let response = setup.rx.recv().await.expect("Failed to receive response");
    match response {
        ChatMessage::ToolResult { id, result } => {
            assert_eq!(id, cmd_id);
            assert!(result.contains("line2"));
            assert!(!result.contains("line1"));
            assert!(!result.contains("line3"));
        }
        _ => panic!("Expected ToolResult"),
    }
}

#[tokio::test]
async fn test_bash_timeout() {
    let mut setup = setup_bash_test().await;
    
    // Test command with timeout
    let cmd_id = Uuid::new_v4();
    let params = json!({
        "command": "sleep 1 && echo 'Done'",
        "timeout": 500  // 500ms timeout, should fail
    });
    
    setup.bash_ref
        .send_message(ToolMessage::Execute {
            id: cmd_id,
            params,
            chat_ref: setup.chat_ref.clone(),
        })
        .expect("Failed to send command");
    
    let response = setup.rx.recv().await.expect("Failed to receive response");
    match response {
        ChatMessage::ToolResult { id, result } => {
            assert_eq!(id, cmd_id);
            // Should timeout
            assert!(result.contains("Error") || result.contains("timed out") || result.contains("Timeout"));
        }
        _ => panic!("Expected ToolResult"),
    }
}

#[tokio::test]
async fn test_bash_working_directory() {
    let mut setup = setup_bash_test().await;
    
    // Create temp directory and work in it
    let cmd_id = Uuid::new_v4();
    let params = json!({
        "command": "mkdir -p /tmp/bash_test_$$ && cd /tmp/bash_test_$$ && pwd && cd .. && rm -rf /tmp/bash_test_$$"
    });
    
    setup.bash_ref
        .send_message(ToolMessage::Execute {
            id: cmd_id,
            params,
            chat_ref: setup.chat_ref.clone(),
        })
        .expect("Failed to send command");
    
    let response = setup.rx.recv().await.expect("Failed to receive response");
    match response {
        ChatMessage::ToolResult { id, result } => {
            assert_eq!(id, cmd_id);
            assert!(result.contains("/tmp/bash_test_"));
        }
        _ => panic!("Expected ToolResult"),
    }
}

#[tokio::test]
async fn test_bash_invalid_params() {
    let mut setup = setup_bash_test().await;
    
    // Test with missing command parameter
    let cmd_id = Uuid::new_v4();
    let params = json!({
        "not_command": "echo test"
    });
    
    setup.bash_ref
        .send_message(ToolMessage::Execute {
            id: cmd_id,
            params,
            chat_ref: setup.chat_ref.clone(),
        })
        .expect("Failed to send command");
    
    let response = setup.rx.recv().await.expect("Failed to receive response");
    match response {
        ChatMessage::ToolResult { id, result } => {
            assert_eq!(id, cmd_id);
            assert!(result.contains("Error"));
        }
        _ => panic!("Expected ToolResult"),
    }
}