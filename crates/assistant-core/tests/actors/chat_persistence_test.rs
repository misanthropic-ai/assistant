use assistant_core::{
    actors::chat_persistence::{ChatPersistenceActor, ChatPersistenceMessage},
    config::Config,
    ractor::{Actor, ActorRef},
};
use std::time::Duration;
use tempfile::TempDir;
use tokio::sync::oneshot;
use uuid::Uuid;

async fn setup_test_actor() -> (ActorRef<ChatPersistenceMessage>, TempDir) {
    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path().join("test.db");
    
    let mut config = Config::default();
    config.api_key = "test-api-key".to_string();
    config.session.database_path = Some(db_path);
    
    // Configure embeddings
    if let Some(model_config) = config.embeddings.models.get_mut("openai-small") {
        model_config.api_key = Some("test-api-key".to_string());
    }
    
    let actor = ChatPersistenceActor::new(config)
        .await
        .expect("Failed to create persistence actor");
    
    let (actor_ref, _) = Actor::spawn(
        None,
        actor,
        ()
    )
    .await
    .expect("Failed to spawn actor");
    
    (actor_ref, temp_dir)
}

#[tokio::test]
async fn test_persist_user_message() {
    let (actor_ref, _temp_dir) = setup_test_actor().await;
    let session_id = Uuid::new_v4().to_string();
    
    // Send a user message
    actor_ref
        .send_message(ChatPersistenceMessage::PersistUserPrompt {
            id: Uuid::new_v4(),
            session_id: session_id.clone(),
            prompt: "Hello, world!".to_string(),
        })
        .expect("Failed to send message");
    
    // Wait for operation to complete
    let (tx, rx) = oneshot::channel();
    actor_ref
        .send_message(ChatPersistenceMessage::WaitForCompletion { reply_to: tx })
        .expect("Failed to send wait message");
    
    tokio::time::timeout(Duration::from_secs(5), rx)
        .await
        .expect("Timeout waiting for completion")
        .expect("Failed to receive completion signal");
}

#[tokio::test]
async fn test_persist_assistant_message() {
    let (actor_ref, _temp_dir) = setup_test_actor().await;
    let session_id = Uuid::new_v4().to_string();
    
    // Send an assistant message
    actor_ref
        .send_message(ChatPersistenceMessage::PersistAssistantResponse {
            id: Uuid::new_v4(),
            session_id: session_id.clone(),
            response: "I'm here to help!".to_string(),
        })
        .expect("Failed to send message");
    
    // Wait for operation to complete
    let (tx, rx) = oneshot::channel();
    actor_ref
        .send_message(ChatPersistenceMessage::WaitForCompletion { reply_to: tx })
        .expect("Failed to send wait message");
    
    tokio::time::timeout(Duration::from_secs(5), rx)
        .await
        .expect("Timeout waiting for completion")
        .expect("Failed to receive completion signal");
}

#[tokio::test]
async fn test_persist_tool_interaction() {
    let (actor_ref, _temp_dir) = setup_test_actor().await;
    let session_id = Uuid::new_v4().to_string();
    
    // Send a tool interaction
    actor_ref
        .send_message(ChatPersistenceMessage::PersistToolInteraction {
            id: Uuid::new_v4(),
            session_id: session_id.clone(),
            tool_name: "memory".to_string(),
            parameters: Some(serde_json::json!({
                "action": "store",
                "content": "test content"
            })),
            result: Some("Stored successfully".to_string()),
        })
        .expect("Failed to send message");
    
    // Wait for operation to complete
    let (tx, rx) = oneshot::channel();
    actor_ref
        .send_message(ChatPersistenceMessage::WaitForCompletion { reply_to: tx })
        .expect("Failed to send wait message");
    
    tokio::time::timeout(Duration::from_secs(5), rx)
        .await
        .expect("Timeout waiting for completion")
        .expect("Failed to receive completion signal");
}

#[tokio::test]
async fn test_multiple_operations_queue() {
    let (actor_ref, _temp_dir) = setup_test_actor().await;
    let session_id = Uuid::new_v4().to_string();
    
    // Send multiple messages quickly
    for i in 0..5 {
        actor_ref
            .send_message(ChatPersistenceMessage::PersistUserPrompt {
                id: Uuid::new_v4(),
                session_id: session_id.clone(),
                prompt: format!("Message {}", i),
            })
            .expect("Failed to send message");
    }
    
    // Check pending count immediately
    let (tx, rx) = oneshot::channel();
    actor_ref
        .send_message(ChatPersistenceMessage::GetPendingCount { reply_to: tx })
        .expect("Failed to send count message");
    
    let count = tokio::time::timeout(Duration::from_secs(1), rx)
        .await
        .expect("Timeout getting count")
        .expect("Failed to receive count");
    
    // Should have some pending operations
    assert!(count > 0, "Expected pending operations, got {}", count);
    
    // Wait for all to complete
    let (tx, rx) = oneshot::channel();
    actor_ref
        .send_message(ChatPersistenceMessage::WaitForCompletion { reply_to: tx })
        .expect("Failed to send wait message");
    
    tokio::time::timeout(Duration::from_secs(10), rx)
        .await
        .expect("Timeout waiting for completion")
        .expect("Failed to receive completion signal");
    
    // Check count again - should be 0
    let (tx, rx) = oneshot::channel();
    actor_ref
        .send_message(ChatPersistenceMessage::GetPendingCount { reply_to: tx })
        .expect("Failed to send count message");
    
    let count = tokio::time::timeout(Duration::from_secs(1), rx)
        .await
        .expect("Timeout getting count")
        .expect("Failed to receive count");
    
    assert_eq!(count, 0, "Expected no pending operations after completion");
}

#[tokio::test]
async fn test_concurrent_sessions() {
    let (actor_ref, _temp_dir) = setup_test_actor().await;
    
    // Create multiple sessions
    let sessions: Vec<String> = (0..3)
        .map(|_| Uuid::new_v4().to_string())
        .collect();
    
    // Send messages for each session concurrently
    for (i, session_id) in sessions.iter().enumerate() {
        actor_ref
            .send_message(ChatPersistenceMessage::PersistUserPrompt {
                id: Uuid::new_v4(),
                session_id: session_id.clone(),
                prompt: format!("Hello from session {}", i),
            })
            .expect("Failed to send message");
        
        actor_ref
            .send_message(ChatPersistenceMessage::PersistAssistantResponse {
                id: Uuid::new_v4(),
                session_id: session_id.clone(),
                response: format!("Response for session {}", i),
            })
            .expect("Failed to send message");
    }
    
    // Wait for all operations to complete
    let (tx, rx) = oneshot::channel();
    actor_ref
        .send_message(ChatPersistenceMessage::WaitForCompletion { reply_to: tx })
        .expect("Failed to send wait message");
    
    tokio::time::timeout(Duration::from_secs(10), rx)
        .await
        .expect("Timeout waiting for completion")
        .expect("Failed to receive completion signal");
}

#[tokio::test]
async fn test_wait_for_completion_when_empty() {
    let (actor_ref, _temp_dir) = setup_test_actor().await;
    
    // Wait for completion when there are no operations
    let (tx, rx) = oneshot::channel();
    actor_ref
        .send_message(ChatPersistenceMessage::WaitForCompletion { reply_to: tx })
        .expect("Failed to send wait message");
    
    // Should complete immediately
    tokio::time::timeout(Duration::from_millis(100), rx)
        .await
        .expect("Should complete immediately when no operations pending")
        .expect("Failed to receive completion signal");
}

#[tokio::test]
async fn test_generate_chat_name() {
    let (actor_ref, _temp_dir) = setup_test_actor().await;
    let session_id = Uuid::new_v4().to_string();
    
    // First message should trigger name generation
    actor_ref
        .send_message(ChatPersistenceMessage::PersistUserPrompt {
            id: Uuid::new_v4(),
            session_id: session_id.clone(),
            prompt: "What is the meaning of life?".to_string(),
        })
        .expect("Failed to send message");
    
    // Give time for name generation to be triggered
    tokio::time::sleep(Duration::from_millis(100)).await;
    
    // Second message should not trigger name generation
    actor_ref
        .send_message(ChatPersistenceMessage::PersistUserPrompt {
            id: Uuid::new_v4(),
            session_id: session_id.clone(),
            prompt: "Another question".to_string(),
        })
        .expect("Failed to send message");
    
    // Wait for operations to complete
    let (tx, rx) = oneshot::channel();
    actor_ref
        .send_message(ChatPersistenceMessage::WaitForCompletion { reply_to: tx })
        .expect("Failed to send wait message");
    
    tokio::time::timeout(Duration::from_secs(5), rx)
        .await
        .expect("Timeout waiting for completion")
        .expect("Failed to receive completion signal");
}

#[tokio::test]
async fn test_error_handling_in_operations() {
    let (actor_ref, temp_dir) = setup_test_actor().await;
    
    // Delete the database to cause errors
    std::fs::remove_file(temp_dir.path().join("test.db")).ok();
    
    // Try to persist a message - should handle the error gracefully
    actor_ref
        .send_message(ChatPersistenceMessage::PersistUserPrompt {
            id: Uuid::new_v4(),
            session_id: "invalid".to_string(),
            prompt: "This might fail".to_string(),
        })
        .expect("Failed to send message");
    
    // Should still be able to wait for completion
    let (tx, rx) = oneshot::channel();
    actor_ref
        .send_message(ChatPersistenceMessage::WaitForCompletion { reply_to: tx })
        .expect("Failed to send wait message");
    
    tokio::time::timeout(Duration::from_secs(5), rx)
        .await
        .expect("Timeout waiting for completion")
        .expect("Failed to receive completion signal");
}