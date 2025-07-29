use anyhow::{Result, anyhow};
use assistant_core::{
    actors::{
        client::{ClientActor, ClientMessage},
        supervisor::SupervisorActor,
    },
    config::Config,
    messages::ChatMessage,
    ractor::{Actor, ActorRef},
};
use tokio::sync::mpsc;

/// Run a simple prompt without tools to test the OpenAI client
pub async fn run_prompt(prompt: String, config_path: Option<&str>) -> Result<()> {
    // Load configuration
    let config = match config_path {
        Some(path) => Config::load(std::path::Path::new(path))?,
        None => Config::load_default().unwrap_or_else(|_| {
            eprintln!("Warning: Could not load config.json, using defaults");
            Config::default()
        }),
    };
    
    // Check if we have an API key
    if config.api_key.is_empty() || config.api_key == "test-api-key" {
        return Err(anyhow!("No API key found. Please set an API key in config.json"));
    }
    
    tracing::info!("Starting prompt runner with model: {}", config.model);
    
    // Create supervisor
    let supervisor = SupervisorActor::new(config.clone());
    let (supervisor_ref, _) = Actor::spawn(Some("supervisor".to_string()), supervisor, config.clone())
        .await
        .map_err(|e| anyhow!("Failed to spawn supervisor: {}", e))?;
    
    // Create client actor
    let client_actor = ClientActor::new(config.clone());
    let (client_ref, _) = Actor::spawn(
        Some("client".to_string()),
        client_actor,
        config.clone(),
    )
    .await
    .map_err(|e| anyhow!("Failed to spawn client: {}", e))?;
    
    // Create a channel to receive responses
    let (tx, mut rx) = mpsc::unbounded_channel();
    
    // Create a simple actor to receive messages
    struct ResponseCollector {
        sender: mpsc::UnboundedSender<String>,
    }
    
    struct ResponseCollectorState;
    
    impl Actor for ResponseCollector {
        type Msg = ChatMessage;
        type State = ResponseCollectorState;
        type Arguments = mpsc::UnboundedSender<String>;
        
        async fn pre_start(
            &self,
            _myself: ActorRef<Self::Msg>,
            _sender: Self::Arguments,
        ) -> Result<Self::State, assistant_core::ractor::ActorProcessingErr> {
            Ok(ResponseCollectorState)
        }
        
        async fn handle(
            &self,
            _myself: ActorRef<Self::Msg>,
            msg: Self::Msg,
            _state: &mut Self::State,
        ) -> Result<(), assistant_core::ractor::ActorProcessingErr> {
            match msg {
                ChatMessage::StreamToken { token } => {
                    let _ = self.sender.send(token);
                }
                ChatMessage::Complete { id: _, response } => {
                    let _ = self.sender.send(format!("\n[COMPLETE] {}", response));
                    let _ = self.sender.send("\n[STREAM_END]".to_string());
                }
                ChatMessage::Error { id: _, error } => {
                    let _ = self.sender.send(format!("\nError: {}", error));
                }
                _ => {}
            }
            Ok(())
        }
    }
    
    let collector = ResponseCollector { sender: tx.clone() };
    let (collector_ref, _) = Actor::spawn(
        Some("collector".to_string()),
        collector,
        tx,
    )
    .await
    .map_err(|e| anyhow!("Failed to spawn collector: {}", e))?;
    
    // Set the chat ref on the client
    client_ref.send_message(ClientMessage::SetChatRef(collector_ref))
        .map_err(|e| anyhow!("Failed to set chat ref: {}", e))?;
    
    // Create user message
    use assistant_core::async_openai::types::{ChatCompletionRequestUserMessage, ChatCompletionRequestMessage};
    let user_message = ChatCompletionRequestMessage::User(
        ChatCompletionRequestUserMessage {
            content: prompt.clone().into(),
            name: None,
        }
    );
    
    // Send the prompt
    client_ref.send_message(ClientMessage::Generate {
        id: assistant_core::uuid::Uuid::new_v4(),
        messages: vec![user_message],
        tools: vec![],
    })
    .map_err(|e| anyhow!("Failed to send message: {}", e))?;
    
    // Collect the response
    println!("\nAssistant: ");
    let mut stream_ended = false;
    while let Some(content) = rx.recv().await {
        if content == "\n[STREAM_END]" {
            stream_ended = true;
            break;
        }
        print!("{}", content);
        // Flush to show streaming output
        use std::io::Write;
        std::io::stdout().flush()?;
    }
    
    if !stream_ended {
        println!("\n\nWarning: Stream did not end properly");
    } else {
        println!(); // Final newline
    }
    
    Ok(())
}