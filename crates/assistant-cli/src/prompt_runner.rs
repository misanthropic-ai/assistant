use anyhow::{Result, anyhow};
use assistant_core::{
    actors::{
        display::cli::CLIDisplayActor,
        chat_persistence::ChatPersistenceMessage,
    },
    config::Config,
    messages::{ChatMessage, DisplayContext},
    ractor::Actor,
    openai_compat::{ChatMessage as OpenAIMessage, UserContent},
};
use tokio::sync::mpsc;
use assistant_core::actor_init;

/// Run a full agent prompt with tool calling support
pub async fn run_agent_prompt(input: String, max_iterations: usize, config_path: Option<&str>) -> Result<()> {
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
    
    tracing::info!("Starting agent with model: {}", config.model);
    
    // Parse input as either string or JSON message array
    let messages = parse_input(&input)?;
    
    // Initialize actor system
    let actors = actor_init::init_actor_system(config).await?;
    
    // Create completion channel
    let (completion_tx, mut completion_rx) = mpsc::unbounded_channel();
    
    // Create CLI display actor
    let cli_display = CLIDisplayActor::new(completion_tx.clone());
    let (display_ref, _) = Actor::spawn(
        Some("cli_display".to_string()),
        cli_display,
        completion_tx,
    )
    .await?;
    
    // Register display actor with chat
    actors.chat.send_message(ChatMessage::RegisterDisplay {
        context: DisplayContext::CLI,
        display_ref,
    })?;
    
    // Send the initial prompt
    let request_id = assistant_core::uuid::Uuid::new_v4();
    
    // If we have multiple messages, we need to send them to the chat actor
    // For now, we'll just take the last user message as the prompt
    let prompt = match messages.last() {
        Some(OpenAIMessage::User { content, .. }) => {
            // Extract text content from the message
            match content {
                UserContent::Text(text) => text.clone(),
                UserContent::Array(_) => {
                    return Err(anyhow!("Array content not supported yet"));
                }
            }
        }
        _ => return Err(anyhow!("No user message found in input")),
    };
    
    actors.chat.send_message(ChatMessage::UserPrompt {
        id: request_id,
        content: assistant_core::messages::UserMessageContent::Text(prompt),
        context: DisplayContext::CLI,
        session_id: None,  // CLI doesn't manage sessions
    })?;
    
    // Wait for completion
    // The tool calling loop happens automatically in ChatActor
    let mut iterations = 0;
    loop {
        tokio::select! {
            _ = completion_rx.recv() => {
                tracing::debug!("Received completion signal");
                break;
            }
            _ = tokio::time::sleep(tokio::time::Duration::from_secs(60)) => {
                iterations += 1;
                if iterations >= max_iterations {
                    println!("\n⚠️  Max iterations ({}) reached. Stopping.", max_iterations);
                    break;
                }
            }
        }
    }
    
    // Wait for persistence operations to complete
    if let Some(persistence_ref) = &actors.persistence {
        tracing::info!("Waiting for persistence operations to complete...");
        
        // Create a channel to receive completion signal
        let (tx, rx) = tokio::sync::oneshot::channel();
        
        // Send wait message
        if let Err(e) = persistence_ref.send_message(ChatPersistenceMessage::WaitForCompletion {
            reply_to: tx,
        }) {
            tracing::warn!("Failed to send wait message to persistence actor: {}", e);
        } else {
            // Wait for completion (with timeout)
            match tokio::time::timeout(tokio::time::Duration::from_secs(5), rx).await {
                Ok(Ok(())) => {
                    tracing::info!("All persistence operations completed");
                }
                Ok(Err(e)) => {
                    tracing::warn!("Failed to receive completion signal: {}", e);
                }
                Err(_) => {
                    tracing::warn!("Timeout waiting for persistence operations");
                }
            }
        }
    }
    
    Ok(())
}

/// Parse input as either a simple string or JSON message array
fn parse_input(input: &str) -> Result<Vec<OpenAIMessage>> {
    let trimmed = input.trim();
    
    // Check if it looks like JSON array
    if trimmed.starts_with('[') {
        // Parse as JSON array
        let json_messages: Vec<assistant_core::serde_json::Value> = assistant_core::serde_json::from_str(trimmed)?;
        
        let mut messages = Vec::new();
        for json_msg in json_messages {
            let role = json_msg.get("role")
                .and_then(|r| r.as_str())
                .ok_or_else(|| anyhow!("Message missing 'role' field"))?;
                
            let content = json_msg.get("content")
                .and_then(|c| c.as_str())
                .ok_or_else(|| anyhow!("Message missing 'content' field"))?;
                
            let message = match role {
                "system" => OpenAIMessage::System {
                    content: content.to_string(),
                    name: json_msg.get("name").and_then(|n| n.as_str()).map(|s| s.to_string()),
                },
                "user" => OpenAIMessage::User {
                    content: UserContent::Text(content.to_string()),
                    name: json_msg.get("name").and_then(|n| n.as_str()).map(|s| s.to_string()),
                },
                "assistant" => OpenAIMessage::Assistant {
                    content: Some(content.to_string()),
                    name: json_msg.get("name").and_then(|n| n.as_str()).map(|s| s.to_string()),
                    tool_calls: None,
                },
                _ => return Err(anyhow!("Unknown message role: {}", role)),
            };
            messages.push(message);
        }
        
        Ok(messages)
    } else {
        // Simple string prompt
        Ok(vec![
            OpenAIMessage::User {
                content: UserContent::Text(input.to_string()),
                name: None,
            }
        ])
    }
}

/// Run a simple prompt without tools to test the OpenAI client
pub async fn _run_simple_prompt(prompt: String, config_path: Option<&str>) -> Result<()> {
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
    let supervisor = assistant_core::actors::supervisor::SupervisorActor::new(config.clone());
    let (_supervisor_ref, _) = Actor::spawn(Some("supervisor".to_string()), supervisor, config.clone())
        .await
        .map_err(|e| anyhow!("Failed to spawn supervisor: {}", e))?;
    
    // Create client actor
    let client_actor = assistant_core::actors::client::ClientActor::new(config.clone());
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
            _myself: assistant_core::ractor::ActorRef<Self::Msg>,
            _sender: Self::Arguments,
        ) -> Result<Self::State, assistant_core::ractor::ActorProcessingErr> {
            Ok(ResponseCollectorState)
        }
        
        async fn handle(
            &self,
            _myself: assistant_core::ractor::ActorRef<Self::Msg>,
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
    use assistant_core::actors::client::ClientMessage;
    client_ref.send_message(ClientMessage::SetChatRef(collector_ref))
        .map_err(|e| anyhow!("Failed to set chat ref: {}", e))?;
    
    // Create user message
    let user_message = OpenAIMessage::User {
        content: UserContent::Text(prompt.clone()),
        name: None,
    };
    
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