use assistant_core::actors::tools::web_search::WebSearchActor;
use assistant_core::messages::ToolMessage;
use assistant_core::messages::ChatMessage;
use assistant_core::config::Config;
use ractor::{Actor, ActorRef};
use serde_json::json;
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

async fn setup_test() -> (Config, ActorRef<ChatMessage>, mpsc::UnboundedReceiver<ChatMessage>) {
    let mut config = Config::default();
    config.api_key = "test-api-key".to_string();
    
    let (tx, rx) = mpsc::unbounded_channel();
    let mock_chat = MockChatActor { sender: tx.clone() };
    let (chat_ref, _) = Actor::spawn(None, mock_chat, tx).await.unwrap();
    
    (config, chat_ref, rx)
}



#[tokio::test]
async fn test_web_search_empty_query() {
    let (config, chat_ref, mut rx) = setup_test().await;
    
    // Create WebSearchActor
    let web_search_actor = WebSearchActor::new(config.clone());
    let (web_search_ref, _) = Actor::spawn(None, web_search_actor, config).await.unwrap();
    
    // Execute search with empty query
    let id = Uuid::new_v4();
    let params = json!({
        "query": "",
        "limit": 5
    });
    
    web_search_ref.send_message(ToolMessage::Execute {
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
            assert!(result.contains("empty"));
        }
        _ => panic!("Expected ToolResult message"),
    }
}

#[tokio::test]
async fn test_web_search_whitespace_query() {
    let (config, chat_ref, mut rx) = setup_test().await;
    
    // Create WebSearchActor
    let web_search_actor = WebSearchActor::new(config.clone());
    let (web_search_ref, _) = Actor::spawn(None, web_search_actor, config).await.unwrap();
    
    // Execute search with whitespace query
    let id = Uuid::new_v4();
    let params = json!({
        "query": "   \t\n   ",
        "limit": 5
    });
    
    web_search_ref.send_message(ToolMessage::Execute {
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
            assert!(result.contains("empty"));
        }
        _ => panic!("Expected ToolResult message"),
    }
}

#[tokio::test]
async fn test_web_search_invalid_parameters() {
    let (config, chat_ref, mut rx) = setup_test().await;
    
    // Create WebSearchActor
    let web_search_actor = WebSearchActor::new(config.clone());
    let (web_search_ref, _) = Actor::spawn(None, web_search_actor, config).await.unwrap();
    
    // Execute with invalid parameters (missing query)
    let id = Uuid::new_v4();
    let params = json!({
        "limit": 5
        // Missing query
    });
    
    web_search_ref.send_message(ToolMessage::Execute {
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



#[tokio::test]
async fn test_web_search_zero_limit() {
    let (config, chat_ref, mut rx) = setup_test().await;
    
    // Create WebSearchActor
    let web_search_actor = WebSearchActor::new(config.clone());
    let (web_search_ref, _) = Actor::spawn(None, web_search_actor, config).await.unwrap();
    
    // Execute search with zero limit
    let id = Uuid::new_v4();
    let params = json!({
        "query": "test",
        "limit": 0
    });
    
    web_search_ref.send_message(ToolMessage::Execute {
        id,
        params,
        chat_ref,
    }).unwrap();
    
    // Wait for response
    let response = rx.recv().await.unwrap();
    match response {
        ChatMessage::ToolResult { id: res_id, result } => {
            assert_eq!(res_id, id);
            // With limit 0, should show no results
            assert!(result.contains("No results found") || result.contains("Total results shown: 0"));
        }
        _ => panic!("Expected ToolResult message"),
    }
}