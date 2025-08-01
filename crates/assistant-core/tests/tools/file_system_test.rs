use assistant_core::{
    actors::tools::{ls::LsActor, read::ReadActor, write::WriteActor, edit::EditActor, glob::GlobActor, grep::GrepActor},
    config::Config,
    messages::{ToolMessage, ChatMessage},
    ractor::{Actor, ActorRef},
};
use serde_json::json;
use tempfile::TempDir;
use tokio::sync::mpsc;
use uuid::Uuid;

struct TestSetup {
    ls_ref: ActorRef<ToolMessage>,
    read_ref: ActorRef<ToolMessage>,
    write_ref: ActorRef<ToolMessage>,
    edit_ref: ActorRef<ToolMessage>,
    glob_ref: ActorRef<ToolMessage>,
    grep_ref: ActorRef<ToolMessage>,
    chat_ref: ActorRef<ChatMessage>,
    rx: mpsc::UnboundedReceiver<ChatMessage>,
    temp_dir: TempDir,
    config: Config,
}

async fn setup_file_system_test() -> TestSetup {
    let temp_dir = TempDir::new().unwrap();
    
    let mut config = Config::default();
    config.api_key = "test-api-key".to_string();
    config.session.workspace_path = Some(temp_dir.path().to_path_buf());
    
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
    
    // Create file system actors
    let ls_actor = LsActor::new(config.clone());
    let (ls_ref, _) = Actor::spawn(
        None,
        ls_actor,
        config.clone(),
    )
    .await
    .expect("Failed to spawn ls actor");
    
    let read_actor = ReadActor::new(config.clone());
    let (read_ref, _) = Actor::spawn(
        None,
        read_actor,
        config.clone(),
    )
    .await
    .expect("Failed to spawn read actor");
    
    let write_actor = WriteActor::new(config.clone());
    let (write_ref, _) = Actor::spawn(
        None,
        write_actor,
        config.clone(),
    )
    .await
    .expect("Failed to spawn write actor");
    
    let edit_actor = EditActor::new(config.clone());
    let (edit_ref, _) = Actor::spawn(
        None,
        edit_actor,
        config.clone(),
    )
    .await
    .expect("Failed to spawn edit actor");
    
    let glob_actor = GlobActor::new(config.clone());
    let (glob_ref, _) = Actor::spawn(
        None,
        glob_actor,
        config.clone(),
    )
    .await
    .expect("Failed to spawn glob actor");
    
    let grep_actor = GrepActor::new(config.clone());
    let (grep_ref, _) = Actor::spawn(
        None,
        grep_actor,
        config.clone(),
    )
    .await
    .expect("Failed to spawn grep actor");
    
    TestSetup {
        ls_ref,
        read_ref,
        write_ref,
        edit_ref,
        glob_ref,
        grep_ref,
        chat_ref,
        rx,
        temp_dir,
        config,
    }
}

#[tokio::test]
async fn test_write_and_read_file() {
    let mut setup = setup_file_system_test().await;
    let file_path = setup.temp_dir.path().join("test.txt");
    
    // Write a file
    let write_id = Uuid::new_v4();
    let params = json!({
        "file_path": file_path.to_string_lossy(),
        "content": "Hello, World!\nThis is a test file."
    });
    
    setup.write_ref
        .send_message(ToolMessage::Execute {
            id: write_id,
            params,
            chat_ref: setup.chat_ref.clone(),
        })
        .expect("Failed to send write message");
    
    let response = setup.rx.recv().await.expect("Failed to receive response");
    match response {
        ChatMessage::ToolResult { id, result } => {
            assert_eq!(id, write_id);
            assert!(result.contains("Successfully created new file"));
        }
        _ => panic!("Expected ToolResult"),
    }
    
    // Read the file
    let read_id = Uuid::new_v4();
    let params = json!({
        "file_path": file_path.to_string_lossy()
    });
    
    setup.read_ref
        .send_message(ToolMessage::Execute {
            id: read_id,
            params,
            chat_ref: setup.chat_ref.clone(),
        })
        .expect("Failed to send read message");
    
    let response = setup.rx.recv().await.expect("Failed to receive response");
    match response {
        ChatMessage::ToolResult { id, result } => {
            assert_eq!(id, read_id);
            assert!(result.contains("Hello, World!"));
            assert!(result.contains("This is a test file."));
        }
        _ => panic!("Expected ToolResult"),
    }
}

#[tokio::test]
async fn test_edit_file() {
    let mut setup = setup_file_system_test().await;
    let file_path = setup.temp_dir.path().join("edit_test.txt");
    
    // First write a file
    let params = json!({
        "file_path": file_path.to_string_lossy(),
        "content": "Line 1\nLine 2\nLine 3\nLine 4"
    });
    
    setup.write_ref
        .send_message(ToolMessage::Execute {
            id: Uuid::new_v4(),
            params,
            chat_ref: setup.chat_ref.clone(),
        })
        .expect("Failed to send write message");
    
    let _ = setup.rx.recv().await;
    
    // Edit the file
    let edit_id = Uuid::new_v4();
    let params = json!({
        "file_path": file_path.to_string_lossy(),
        "old_string": "Line 2",
        "new_string": "Modified Line 2"
    });
    
    setup.edit_ref
        .send_message(ToolMessage::Execute {
            id: edit_id,
            params,
            chat_ref: setup.chat_ref.clone(),
        })
        .expect("Failed to send edit message");
    
    let response = setup.rx.recv().await.expect("Failed to receive response");
    match response {
        ChatMessage::ToolResult { id, result } => {
            assert_eq!(id, edit_id);
            assert!(result.contains("Successfully edited file"));
        }
        _ => panic!("Expected ToolResult"),
    }
    
    // Read to verify
    let params = json!({
        "file_path": file_path.to_string_lossy()
    });
    
    setup.read_ref
        .send_message(ToolMessage::Execute {
            id: Uuid::new_v4(),
            params,
            chat_ref: setup.chat_ref.clone(),
        })
        .expect("Failed to send read message");
    
    let response = setup.rx.recv().await.expect("Failed to receive response");
    match response {
        ChatMessage::ToolResult { result, .. } => {
            assert!(result.contains("Modified Line 2"));
            assert!(!result.contains("Line 2\n"));
        }
        _ => panic!("Expected ToolResult"),
    }
}

#[tokio::test]
async fn test_ls_directory() {
    let mut setup = setup_file_system_test().await;
    
    // Create some files
    let files = vec!["file1.txt", "file2.py", "file3.rs"];
    for file in &files {
        let file_path = setup.temp_dir.path().join(file);
        let params = json!({
            "file_path": file_path.to_string_lossy(),
            "content": format!("Content of {}", file)
        });
        
        setup.write_ref
            .send_message(ToolMessage::Execute {
                id: Uuid::new_v4(),
                params,
                chat_ref: setup.chat_ref.clone(),
            })
            .expect("Failed to send write message");
        
        let _ = setup.rx.recv().await;
    }
    
    // Create a subdirectory
    let sub_dir = setup.temp_dir.path().join("subdir");
    std::fs::create_dir(&sub_dir).expect("Failed to create directory");
    
    // List directory
    let ls_id = Uuid::new_v4();
    let params = json!({
        "path": setup.temp_dir.path().to_string_lossy()
    });
    
    setup.ls_ref
        .send_message(ToolMessage::Execute {
            id: ls_id,
            params,
            chat_ref: setup.chat_ref.clone(),
        })
        .expect("Failed to send ls message");
    
    let response = setup.rx.recv().await.expect("Failed to receive response");
    match response {
        ChatMessage::ToolResult { id, result } => {
            assert_eq!(id, ls_id);
            for file in &files {
                assert!(result.contains(file));
            }
            assert!(result.contains("subdir"));
        }
        _ => panic!("Expected ToolResult"),
    }
}

#[tokio::test]
async fn test_glob_pattern() {
    let mut setup = setup_file_system_test().await;
    
    // Create files with different extensions
    let files = vec![
        "src/main.rs",
        "src/lib.rs",
        "tests/test1.rs",
        "tests/test2.rs",
        "docs/readme.md",
        "docs/guide.md"
    ];
    
    for file in &files {
        let file_path = setup.temp_dir.path().join(file);
        std::fs::create_dir_all(file_path.parent().unwrap()).ok();
        
        let params = json!({
            "file_path": file_path.to_string_lossy(),
            "content": format!("// {}", file)
        });
        
        setup.write_ref
            .send_message(ToolMessage::Execute {
                id: Uuid::new_v4(),
                params,
                chat_ref: setup.chat_ref.clone(),
            })
            .expect("Failed to send write message");
        
        let _ = setup.rx.recv().await;
    }
    
    // Glob for Rust files
    let glob_id = Uuid::new_v4();
    let params = json!({
        "pattern": "**/*.rs",
        "path": setup.temp_dir.path().to_string_lossy()
    });
    
    setup.glob_ref
        .send_message(ToolMessage::Execute {
            id: glob_id,
            params,
            chat_ref: setup.chat_ref.clone(),
        })
        .expect("Failed to send glob message");
    
    let response = setup.rx.recv().await.expect("Failed to receive response");
    match response {
        ChatMessage::ToolResult { id, result } => {
            assert_eq!(id, glob_id);
            assert!(result.contains("main.rs"));
            assert!(result.contains("lib.rs"));
            assert!(result.contains("test1.rs"));
            assert!(result.contains("test2.rs"));
            assert!(!result.contains("readme.md"));
            assert!(!result.contains("guide.md"));
        }
        _ => panic!("Expected ToolResult"),
    }
}

#[tokio::test]
async fn test_grep_search() {
    let mut setup = setup_file_system_test().await;
    
    // Create files with content
    let files = vec![
        ("code.rs", "fn main() {\n    println!(\"Hello, world!\");\n}"),
        ("test.rs", "#[test]\nfn test_hello() {\n    assert_eq!(\"hello\", \"hello\");\n}"),
        ("lib.rs", "pub fn hello() -> &'static str {\n    \"Hello\"\n}")
    ];
    
    for (file, content) in &files {
        let file_path = setup.temp_dir.path().join(file);
        let params = json!({
            "file_path": file_path.to_string_lossy(),
            "content": content
        });
        
        setup.write_ref
            .send_message(ToolMessage::Execute {
                id: Uuid::new_v4(),
                params,
                chat_ref: setup.chat_ref.clone(),
            })
            .expect("Failed to send write message");
        
        let _ = setup.rx.recv().await;
    }
    
    // Grep for "hello"
    let grep_id = Uuid::new_v4();
    let params = json!({
        "pattern": "hello",
        "path": setup.temp_dir.path().to_string_lossy(),
        "-i": true  // Case insensitive
    });
    
    setup.grep_ref
        .send_message(ToolMessage::Execute {
            id: grep_id,
            params,
            chat_ref: setup.chat_ref.clone(),
        })
        .expect("Failed to send grep message");
    
    let response = setup.rx.recv().await.expect("Failed to receive response");
    match response {
        ChatMessage::ToolResult { id, result } => {
            assert_eq!(id, grep_id);
            assert!(result.contains("code.rs"));
            assert!(result.contains("test.rs"));
            assert!(result.contains("lib.rs"));
        }
        _ => panic!("Expected ToolResult"),
    }
}

#[tokio::test]
async fn test_file_not_found_errors() {
    let mut setup = setup_file_system_test().await;
    let non_existent = setup.temp_dir.path().join("non_existent.txt");
    
    // Try to read non-existent file
    let read_id = Uuid::new_v4();
    let params = json!({
        "file_path": non_existent.to_string_lossy()
    });
    
    setup.read_ref
        .send_message(ToolMessage::Execute {
            id: read_id,
            params,
            chat_ref: setup.chat_ref.clone(),
        })
        .expect("Failed to send read message");
    
    let response = setup.rx.recv().await.expect("Failed to receive response");
    match response {
        ChatMessage::ToolResult { id, result } => {
            assert_eq!(id, read_id);
            assert!(result.contains("Error") || result.contains("not found"));
        }
        _ => panic!("Expected ToolResult"),
    }
    
    // Try to edit non-existent file
    let edit_id = Uuid::new_v4();
    let params = json!({
        "file_path": non_existent.to_string_lossy(),
        "old_string": "old",
        "new_string": "new"
    });
    
    setup.edit_ref
        .send_message(ToolMessage::Execute {
            id: edit_id,
            params,
            chat_ref: setup.chat_ref.clone(),
        })
        .expect("Failed to send edit message");
    
    let response = setup.rx.recv().await.expect("Failed to receive response");
    match response {
        ChatMessage::ToolResult { id, result } => {
            assert_eq!(id, edit_id);
            assert!(result.contains("Error") || result.contains("not found"));
        }
        _ => panic!("Expected ToolResult"),
    }
}

#[tokio::test]
#[ignore = "Workspace validation not yet implemented - validate_path_access is a placeholder"]
async fn test_workspace_security() {
    let mut setup = setup_file_system_test().await;
    
    // Try to write outside workspace
    let outside_path = "/tmp/outside_workspace.txt";
    let write_id = Uuid::new_v4();
    let params = json!({
        "file_path": outside_path,
        "content": "Should not be written"
    });
    
    setup.write_ref
        .send_message(ToolMessage::Execute {
            id: write_id,
            params,
            chat_ref: setup.chat_ref.clone(),
        })
        .expect("Failed to send write message");
    
    let response = setup.rx.recv().await.expect("Failed to receive response");
    match response {
        ChatMessage::ToolResult { id, result } => {
            assert_eq!(id, write_id);
            // Should error when trying to write outside workspace
            assert!(result.contains("Error"));
        }
        _ => panic!("Expected ToolResult"),
    }
}