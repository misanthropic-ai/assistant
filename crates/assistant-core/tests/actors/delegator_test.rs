use assistant_core::{
    actors::delegator::DelegatorActor,
    actors::client::ClientMessage,
    config::{Config, tool_config::ToolConfig},
    messages::{DelegatorMessage, ChatMessage},
    ractor::{Actor, ActorRef},
};
use serde_json::json;
use std::collections::HashMap;
use tokio::sync::mpsc;
use uuid::Uuid;

struct TestSetup {
    delegator_ref: ActorRef<DelegatorMessage>,
    chat_ref: ActorRef<ChatMessage>,
    rx: mpsc::UnboundedReceiver<ChatMessage>,
    client_rx: mpsc::UnboundedReceiver<ClientMessage>,
}

async fn setup_delegator_test() -> TestSetup {
    let mut config = Config::default();
    config.api_key = "test-api-key".to_string();
    
    // Configure tool delegation
    let mut tools = HashMap::new();
    tools.insert("web_search".to_string(), ToolConfig {
        delegate: Some(true),
        api_key: Some("delegated-api-key".to_string()),
        model: Some("gpt-4-turbo".to_string()),
        system_prompt: Some("You are a web search specialist.".to_string()),
        api_base: None,
        temperature: Some(0.3),
        timeout: None,
        max_tokens: None,
    });
    config.tools = Some(tools);
    
    // Create channels
    let (chat_tx, rx) = mpsc::unbounded_channel();
    let (client_tx, client_rx) = mpsc::unbounded_channel();
    
    // Mock chat actor
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
    
    // Mock client actor
    struct MockClientActor {
        tx: mpsc::UnboundedSender<ClientMessage>,
    }
    
    impl Actor for MockClientActor {
        type Msg = ClientMessage;
        type State = ();
        type Arguments = mpsc::UnboundedSender<ClientMessage>;
        
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
    
    let mock_chat = MockChatActor { tx: chat_tx.clone() };
    let (chat_ref, _) = Actor::spawn(
        None,
        mock_chat,
        chat_tx,
    )
    .await
    .expect("Failed to spawn mock chat");
    
    let mock_client = MockClientActor { tx: client_tx.clone() };
    let (client_ref, _) = Actor::spawn(
        None,
        mock_client,
        client_tx,
    )
    .await
    .expect("Failed to spawn mock client");
    
    // Create delegator
    let delegator = DelegatorActor::new(config.clone());
    let (delegator_ref, _) = Actor::spawn(
        None,
        delegator,
        (config, chat_ref.clone(), client_ref),
    )
    .await
    .expect("Failed to spawn delegator");
    
    TestSetup {
        delegator_ref,
        chat_ref,
        rx,
        client_rx,
    }
}

#[tokio::test]
async fn test_delegator_non_delegated_tool() {
    let mut setup = setup_delegator_test().await;
    
    // Request for non-delegated tool should return UseDefaultClient
    let req_id = Uuid::new_v4();
    let (tx, rx) = tokio::sync::oneshot::channel();
    
    setup.delegator_ref
        .send_message(DelegatorMessage::CheckDelegation {
            tool_name: "ls".to_string(),
            reply: tx,
        })
        .expect("Failed to send message");
    
    let response = rx.await.expect("Failed to receive response");
    match response {
        DelegatorMessage::UseDefaultClient => {
            // Expected - ls is not configured for delegation
        }
        _ => panic!("Expected UseDefaultClient"),
    }
}

#[tokio::test]
async fn test_delegator_delegated_tool() {
    let mut setup = setup_delegator_test().await;
    
    // Request for delegated tool should return DelegateToClient
    let (tx, rx) = tokio::sync::oneshot::channel();
    
    setup.delegator_ref
        .send_message(DelegatorMessage::CheckDelegation {
            tool_name: "web_search".to_string(),
            reply: tx,
        })
        .expect("Failed to send message");
    
    let response = rx.await.expect("Failed to receive response");
    match response {
        DelegatorMessage::DelegateToClient { client_ref } => {
            // Should have created a dedicated client
            assert!(client_ref.get_name().is_some());
        }
        _ => panic!("Expected DelegateToClient"),
    }
}

#[tokio::test]
async fn test_delegator_caches_clients() {
    let mut setup = setup_delegator_test().await;
    
    // First request
    let (tx1, rx1) = tokio::sync::oneshot::channel();
    setup.delegator_ref
        .send_message(DelegatorMessage::CheckDelegation {
            tool_name: "web_search".to_string(),
            reply: tx1,
        })
        .expect("Failed to send message");
    
    let client1 = match rx1.await.expect("Failed to receive response") {
        DelegatorMessage::DelegateToClient { client_ref } => client_ref,
        _ => panic!("Expected DelegateToClient"),
    };
    
    // Second request - should return same client
    let (tx2, rx2) = tokio::sync::oneshot::channel();
    setup.delegator_ref
        .send_message(DelegatorMessage::CheckDelegation {
            tool_name: "web_search".to_string(),
            reply: tx2,
        })
        .expect("Failed to send message");
    
    let client2 = match rx2.await.expect("Failed to receive response") {
        DelegatorMessage::DelegateToClient { client_ref } => client_ref,
        _ => panic!("Expected DelegateToClient"),
    };
    
    // Should be the same client actor
    assert_eq!(client1.get_name(), client2.get_name());
}

#[tokio::test]
async fn test_delegator_tool_request() {
    let mut setup = setup_delegator_test().await;
    
    // Send a tool request that should be delegated
    let req_id = Uuid::new_v4();
    setup.delegator_ref
        .send_message(DelegatorMessage::ToolRequest {
            id: req_id,
            tool_name: "web_search".to_string(),
            arguments: json!({"query": "test search"}),
            messages: vec![
                json!({
                    "role": "user",
                    "content": "Search for test"
                })
            ],
        })
        .expect("Failed to send message");
    
    // Should receive a client message for the delegated request
    let client_msg = setup.client_rx.recv().await.expect("Failed to receive client message");
    match client_msg {
        ClientMessage::Complete { id, messages, tools } => {
            assert_eq!(id, req_id);
            // Should include the system prompt for web search
            assert!(messages.iter().any(|m| 
                m.get("role") == Some(&json!("system")) &&
                m.get("content").and_then(|c| c.as_str()).map(|s| s.contains("web search")).unwrap_or(false)
            ));
        }
        _ => panic!("Expected Complete message"),
    }
}

#[tokio::test]
async fn test_delegator_multiple_tools() {
    let mut setup = setup_delegator_test().await;
    
    // Add another delegated tool
    let mut config = Config::default();
    config.api_key = "test-api-key".to_string();
    let mut tools = HashMap::new();
    tools.insert("web_search".to_string(), ToolConfig {
        delegate: Some(true),
        api_key: Some("search-key".to_string()),
        model: Some("gpt-4".to_string()),
        system_prompt: Some("Search specialist".to_string()),
        api_base: None,
        temperature: None,
        timeout: None,
        max_tokens: None,
    });
    tools.insert("code_analysis".to_string(), ToolConfig {
        delegate: Some(true),
        api_key: Some("code-key".to_string()),
        model: Some("claude-3".to_string()),
        system_prompt: Some("Code analyst".to_string()),
        api_base: None,
        temperature: None,
        timeout: None,
        max_tokens: None,
    });
    config.tools = Some(tools);
    
    // Create new delegator with multiple tools
    let delegator = DelegatorActor::new(config.clone());
    let (delegator_ref, _) = Actor::spawn(
        None,
        delegator,
        (config, setup.chat_ref.clone(), setup.delegator_ref.clone()), // Reuse refs for simplicity
    )
    .await
    .expect("Failed to spawn delegator");
    
    // Check both tools get different clients
    let (tx1, rx1) = tokio::sync::oneshot::channel();
    delegator_ref
        .send_message(DelegatorMessage::CheckDelegation {
            tool_name: "web_search".to_string(),
            reply: tx1,
        })
        .expect("Failed to send message");
    
    let (tx2, rx2) = tokio::sync::oneshot::channel();
    delegator_ref
        .send_message(DelegatorMessage::CheckDelegation {
            tool_name: "code_analysis".to_string(),
            reply: tx2,
        })
        .expect("Failed to send message");
    
    let client1 = match rx1.await.expect("Failed to receive response") {
        DelegatorMessage::DelegateToClient { client_ref } => client_ref,
        _ => panic!("Expected DelegateToClient"),
    };
    
    let client2 = match rx2.await.expect("Failed to receive response") {
        DelegatorMessage::DelegateToClient { client_ref } => client_ref,
        _ => panic!("Expected DelegateToClient"),
    };
    
    // Should be different clients
    assert_ne!(client1.get_name(), client2.get_name());
}

#[tokio::test]
async fn test_delegator_disabled_delegation() {
    let mut config = Config::default();
    config.api_key = "test-api-key".to_string();
    
    // Configure tool with delegate = false
    let mut tools = HashMap::new();
    tools.insert("test_tool".to_string(), ToolConfig {
        delegate: Some(false), // Explicitly disabled
        api_key: Some("key".to_string()),
        model: Some("model".to_string()),
        system_prompt: None,
        api_base: None,
        temperature: None,
        timeout: None,
        max_tokens: None,
    });
    config.tools = Some(tools);
    
    let (chat_tx, _) = mpsc::unbounded_channel();
    let mock_chat = MockChatActor { tx: chat_tx.clone() };
    let (chat_ref, _) = Actor::spawn(
        None,
        mock_chat,
        chat_tx,
    )
    .await
    .expect("Failed to spawn mock chat");
    
    let delegator = DelegatorActor::new(config.clone());
    let (delegator_ref, _) = Actor::spawn(
        None,
        delegator,
        (config, chat_ref, chat_ref.clone()), // Dummy refs
    )
    .await
    .expect("Failed to spawn delegator");
    
    let (tx, rx) = tokio::sync::oneshot::channel();
    delegator_ref
        .send_message(DelegatorMessage::CheckDelegation {
            tool_name: "test_tool".to_string(),
            reply: tx,
        })
        .expect("Failed to send message");
    
    match rx.await.expect("Failed to receive response") {
        DelegatorMessage::UseDefaultClient => {
            // Expected - delegation is explicitly disabled
        }
        _ => panic!("Expected UseDefaultClient for disabled delegation"),
    }
}

// Helper for mock chat actor
struct MockChatActor {
    tx: mpsc::UnboundedSender<ChatMessage>,
}