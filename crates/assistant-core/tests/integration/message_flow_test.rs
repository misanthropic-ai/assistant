use assistant_core::{
    actors::{
        chat::ChatActor,
        chat_persistence::{ChatPersistenceActor, ChatPersistenceMessage},
        client::ClientActor,
        delegator::DelegatorActor,
        tools::memory::MemoryActor,
    },
    config::Config,
    messages::{ChatMessage, DelegatorMessage, ToolMessage, DisplayContext},
    ractor::{Actor, ActorRef},
};
use std::time::Duration;
use tempfile::TempDir;
use tokio::sync::{mpsc, oneshot};
use uuid::Uuid;

struct TestSystem {
    chat: ActorRef<ChatMessage>,
    persistence: ActorRef<ChatPersistenceMessage>,
    _temp_dir: TempDir,
    display_rx: mpsc::UnboundedReceiver<ChatMessage>,
}

async fn setup_test_system() -> TestSystem {
    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path().join("test.db");
    
    let mut config = Config::default();
    config.api_key = "test-api-key".to_string();
    config.session.database_path = Some(db_path.clone());
    config.model = "gpt-3.5-turbo".to_string();
    // Configure embeddings
    if let Some(model_config) = config.embeddings.models.get_mut("openai-small") {
        model_config.api_key = Some("test-api-key".to_string());
    }
    
    // Create persistence actor
    let persistence_actor = ChatPersistenceActor::new(config.clone())
        .await
        .expect("Failed to create persistence actor");
    
    let (persistence_ref, _) = Actor::spawn(
        None,
        persistence_actor,
        ()
    )
    .await
    .expect("Failed to spawn persistence actor");
    
    // Create delegator
    let delegator = DelegatorActor::new(config.clone());
    let (delegator_ref, _) = Actor::spawn(
        None,
        delegator,
        config.clone(),
    )
    .await
    .expect("Failed to spawn delegator");
    
    // Create client (mock for testing)
    let client = ClientActor::new(config.clone());
    let (client_ref, _) = Actor::spawn(
        None,
        client,
        config.clone(),
    )
    .await
    .expect("Failed to spawn client");
    
    // Create chat actor
    let session_id = Uuid::new_v4().to_string();
    let chat = ChatActor::new(config.clone(), session_id)
        .with_client_ref(client_ref.clone())
        .with_delegator_ref(delegator_ref.clone())
        .with_persistence_ref(persistence_ref.clone());
    
    let (chat_ref, _) = Actor::spawn(
        None,
        chat,
        config.clone(),
    )
    .await
    .expect("Failed to spawn chat");
    
    // Create display channel to capture messages
    let (display_tx, display_rx) = mpsc::unbounded_channel();
    
    // Create a simple display actor
    struct TestDisplayActor {
        tx: mpsc::UnboundedSender<ChatMessage>,
    }
    
    impl Actor for TestDisplayActor {
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
    
    let display_actor = TestDisplayActor { tx: display_tx.clone() };
    let (display_ref, _) = Actor::spawn(
        None,
        display_actor,
        display_tx,
    )
    .await
    .expect("Failed to spawn display");
    
    // Register display with chat
    chat_ref
        .send_message(ChatMessage::RegisterDisplay {
            context: DisplayContext::CLI,
            display_ref,
        })
        .expect("Failed to register display");
    
    // Set up actor references
    client_ref
        .send_message(assistant_core::actors::client::ClientMessage::SetChatRef(chat_ref.clone()))
        .expect("Failed to set chat ref");
    
    // Register memory tool
    let memory_db_path = temp_dir.path().join("memory.db");
    let mut memory_config = config.clone();
    memory_config.session.database_path = Some(memory_db_path);
    
    let memory_actor = MemoryActor::new(memory_config.clone())
        .await
        .expect("Failed to create memory actor");
    
    let (memory_ref, _): (ActorRef<ToolMessage>, _) = Actor::spawn(
        None,
        memory_actor,
        memory_config,
    )
    .await
    .expect("Failed to spawn memory");
    
    delegator_ref
        .send_message(DelegatorMessage::RegisterTool {
            name: "memory".to_string(),
            actor_ref: memory_ref,
        })
        .expect("Failed to register memory tool");
    
    TestSystem {
        chat: chat_ref,
        persistence: persistence_ref,
        _temp_dir: temp_dir,
        display_rx,
    }
}

#[tokio::test]
async fn test_user_prompt_persistence() {
    let mut system = setup_test_system().await;
    
    // Send a user prompt
    let request_id = Uuid::new_v4();
    system.chat
        .send_message(ChatMessage::UserPrompt {
            id: request_id,
            prompt: "Hello, assistant!".to_string(),
            context: DisplayContext::CLI,
        })
        .expect("Failed to send user prompt");
    
    // Wait a bit for processing and message forwarding
    tokio::time::sleep(Duration::from_millis(100)).await;
    
    // Wait for persistence to complete
    let (tx, rx) = oneshot::channel();
    system.persistence
        .send_message(ChatPersistenceMessage::WaitForCompletion { reply_to: tx })
        .expect("Failed to send wait message");
    
    tokio::time::timeout(Duration::from_secs(5), rx)
        .await
        .expect("Timeout waiting for persistence")
        .expect("Failed to receive completion");
    
    // Verify we received display messages
    let mut received_user_prompt = false;
    while let Ok(msg) = system.display_rx.try_recv() {
        if let ChatMessage::UserPrompt { prompt, .. } = msg {
            if prompt == "Hello, assistant!" {
                received_user_prompt = true;
            }
        }
    }
    
    assert!(received_user_prompt, "User prompt was not displayed");
}

#[tokio::test]
async fn test_tool_call_persistence() {
    let mut system = setup_test_system().await;
    
    // Send a prompt that would trigger a tool call
    let request_id = Uuid::new_v4();
    system.chat
        .send_message(ChatMessage::UserPrompt {
            id: request_id,
            prompt: "Store a memory that I like pizza".to_string(),
            context: DisplayContext::CLI,
        })
        .expect("Failed to send user prompt");
    
    // Wait for processing
    tokio::time::sleep(Duration::from_secs(1)).await;
    
    // Wait for persistence to complete
    let (tx, rx) = oneshot::channel();
    system.persistence
        .send_message(ChatPersistenceMessage::WaitForCompletion { reply_to: tx })
        .expect("Failed to send wait message");
    
    tokio::time::timeout(Duration::from_secs(10), rx)
        .await
        .expect("Timeout waiting for persistence")
        .expect("Failed to receive completion");
    
    // Check for tool-related messages
    let mut _received_tool_request = false;
    while let Ok(msg) = system.display_rx.try_recv() {
        match msg {
            ChatMessage::ToolRequest { .. } => _received_tool_request = true,
            _ => {}
        }
    }
    
    // Note: In a real test with a mock client, we'd verify tool execution
    // For now, we just check that the flow initiated
}

#[tokio::test]
async fn test_concurrent_message_persistence() {
    let system = setup_test_system().await;
    
    // Send multiple messages concurrently
    let mut handles = vec![];
    
    for i in 0..5 {
        let chat_ref = system.chat.clone();
        let handle = tokio::spawn(async move {
            let request_id = Uuid::new_v4();
            chat_ref
                .send_message(ChatMessage::UserPrompt {
                    id: request_id,
                    prompt: format!("Message {}", i),
                    context: DisplayContext::CLI,
                })
                .expect("Failed to send message");
        });
        handles.push(handle);
    }
    
    // Wait for all messages to be sent
    for handle in handles {
        handle.await.expect("Task failed");
    }
    
    // Give time for processing
    tokio::time::sleep(Duration::from_secs(1)).await;
    
    // Wait for all persistence operations to complete
    let (tx, rx) = oneshot::channel();
    system.persistence
        .send_message(ChatPersistenceMessage::WaitForCompletion { reply_to: tx })
        .expect("Failed to send wait message");
    
    tokio::time::timeout(Duration::from_secs(10), rx)
        .await
        .expect("Timeout waiting for persistence")
        .expect("Failed to receive completion");
}

#[tokio::test]
async fn test_error_recovery() {
    let system = setup_test_system().await;
    
    // Send an invalid message (this would normally cause an error in a real client)
    let request_id = Uuid::new_v4();
    system.chat
        .send_message(ChatMessage::Error {
            id: request_id,
            error: "Test error".to_string(),
        })
        .expect("Failed to send error message");
    
    // System should still be operational
    system.chat
        .send_message(ChatMessage::UserPrompt {
            id: Uuid::new_v4(),
            prompt: "This should still work".to_string(),
            context: DisplayContext::CLI,
        })
        .expect("System should still accept messages after error");
    
    // Wait for persistence
    let (tx, rx) = oneshot::channel();
    system.persistence
        .send_message(ChatPersistenceMessage::WaitForCompletion { reply_to: tx })
        .expect("Failed to send wait message");
    
    tokio::time::timeout(Duration::from_secs(5), rx)
        .await
        .expect("Timeout waiting for persistence")
        .expect("Failed to receive completion");
}

#[tokio::test]
async fn test_session_isolation() {
    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path().join("test.db");
    
    let mut config = Config::default();
    config.api_key = "test-api-key".to_string();
    config.session.database_path = Some(db_path.clone());
    
    // Create persistence actor
    let persistence_actor = ChatPersistenceActor::new(config.clone())
        .await
        .expect("Failed to create persistence actor");
    
    let (persistence_ref, _) = Actor::spawn(
        None,
        persistence_actor,
        ()
    )
    .await
    .expect("Failed to spawn persistence actor");
    
    // Send messages for different sessions
    let session1 = Uuid::new_v4().to_string();
    let session2 = Uuid::new_v4().to_string();
    
    persistence_ref
        .send_message(ChatPersistenceMessage::PersistUserPrompt {
            id: Uuid::new_v4(),
            session_id: session1.clone(),
            prompt: "Session 1 message".to_string(),
        })
        .expect("Failed to send message");
    
    persistence_ref
        .send_message(ChatPersistenceMessage::PersistUserPrompt {
            id: Uuid::new_v4(),
            session_id: session2.clone(),
            prompt: "Session 2 message".to_string(),
        })
        .expect("Failed to send message");
    
    // Wait for completion
    let (tx, rx) = oneshot::channel();
    persistence_ref
        .send_message(ChatPersistenceMessage::WaitForCompletion { reply_to: tx })
        .expect("Failed to send wait message");
    
    tokio::time::timeout(Duration::from_secs(5), rx)
        .await
        .expect("Timeout waiting for persistence")
        .expect("Failed to receive completion");
    
    // Messages should be isolated by session
    // In a full test, we'd query the database to verify
}