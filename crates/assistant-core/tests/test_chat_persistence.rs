#[cfg(test)]
mod chat_persistence_tests {
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
        
        println!("✅ User message persisted successfully");
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
        
        println!("✅ Assistant message persisted successfully");
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
        
        println!("🔄 Pending operations: {}", count);
        assert!(count > 0, "Expected pending operations");
        
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
        
        println!("✅ All operations completed. Pending: {}", count);
        assert_eq!(count, 0, "Expected no pending operations");
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
        let start = std::time::Instant::now();
        tokio::time::timeout(Duration::from_millis(100), rx)
            .await
            .expect("Should complete immediately when no operations pending")
            .expect("Failed to receive completion signal");
        
        let elapsed = start.elapsed();
        println!("✅ Empty queue completed in {:?}", elapsed);
        assert!(elapsed < Duration::from_millis(50), "Should complete quickly");
    }

    #[tokio::test]
    async fn test_concurrent_sessions() {
        let (actor_ref, _temp_dir) = setup_test_actor().await;
        
        // Create multiple sessions
        let sessions: Vec<String> = (0..3)
            .map(|_| Uuid::new_v4().to_string())
            .collect();
        
        // Send messages for each session concurrently
        let mut handles = vec![];
        
        for (i, session_id) in sessions.iter().enumerate() {
            let actor_ref_clone = actor_ref.clone();
            let session_id_clone = session_id.clone();
            let i_clone = i;
            
            let handle = tokio::spawn(async move {
                // User message
                actor_ref_clone
                    .send_message(ChatPersistenceMessage::PersistUserPrompt {
                        id: Uuid::new_v4(),
                        session_id: session_id_clone.clone(),
                        prompt: format!("Hello from session {}", i_clone),
                    })
                    .expect("Failed to send message");
                
                // Assistant response
                actor_ref_clone
                    .send_message(ChatPersistenceMessage::PersistAssistantResponse {
                        id: Uuid::new_v4(),
                        session_id: session_id_clone,
                        response: format!("Response for session {}", i_clone),
                    })
                    .expect("Failed to send message");
            });
            
            handles.push(handle);
        }
        
        // Wait for all tasks to complete
        for handle in handles {
            handle.await.expect("Task failed");
        }
        
        // Wait for all persistence operations to complete
        let (tx, rx) = oneshot::channel();
        actor_ref
            .send_message(ChatPersistenceMessage::WaitForCompletion { reply_to: tx })
            .expect("Failed to send wait message");
        
        tokio::time::timeout(Duration::from_secs(10), rx)
            .await
            .expect("Timeout waiting for completion")
            .expect("Failed to receive completion signal");
        
        println!("✅ All {} sessions persisted successfully", sessions.len());
    }
}