use anyhow::{Result, anyhow};
use assistant_core::{
    actors::tools::*,
    config::Config,
    messages::{ToolMessage, ChatMessage},
    ractor::{Actor, ActorRef},
    serde_json::Value,
    uuid::Uuid,
};
use tokio::sync::mpsc;

/// Simple chat actor that collects tool results
struct SimpleChatActor {
    sender: mpsc::UnboundedSender<String>,
}

struct SimpleChatState;

impl Actor for SimpleChatActor {
    type Msg = ChatMessage;
    type State = SimpleChatState;
    type Arguments = mpsc::UnboundedSender<String>;
    
    async fn pre_start(
        &self,
        _myself: ActorRef<Self::Msg>,
        _sender: Self::Arguments,
    ) -> Result<Self::State, assistant_core::ractor::ActorProcessingErr> {
        Ok(SimpleChatState)
    }
    
    async fn handle(
        &self,
        _myself: ActorRef<Self::Msg>,
        msg: Self::Msg,
        _state: &mut Self::State,
    ) -> Result<(), assistant_core::ractor::ActorProcessingErr> {
        match msg {
            ChatMessage::ToolResult { id: _, result } => {
                // Send the result through the channel
                let _ = self.sender.send(result);
            }
            _ => {
                // Ignore other message types
            }
        }
        Ok(())
    }
}

/// Run a tool directly and return its result
pub async fn run_tool(tool_name: &str, params: Value, config_path: Option<&str>) -> Result<String> {
    // Load configuration
    let config = match config_path {
        Some(path) => Config::load(std::path::Path::new(path))?,
        None => Config::load_default().unwrap_or_else(|_| {
            eprintln!("Warning: Could not load config.json, using defaults");
            Config::default()
        }),
    };
    
    // Check if tool is enabled
    if !config.is_tool_enabled(tool_name) {
        return Err(anyhow!("Tool '{}' is disabled in configuration", tool_name));
    }
    
    // Create a channel to receive results
    let (tx, mut rx) = mpsc::unbounded_channel();
    
    // Create the simple chat actor
    let chat_actor = SimpleChatActor { sender: tx.clone() };
    let (chat_ref, _) = Actor::spawn(
        Some("simple-chat".to_string()),
        chat_actor,
        tx,
    ).await?;
    
    // Create the tool actor based on the name
    let tool_ref: ActorRef<ToolMessage> = match tool_name {
        "ls" => {
            let actor = LsActor::new(config.clone());
            let (actor_ref, _) = Actor::spawn(
                Some(tool_name.to_string()),
                actor,
                config.clone(),
            ).await?;
            actor_ref
        }
        "read" => {
            let actor = ReadActor::new(config.clone());
            let (actor_ref, _) = Actor::spawn(
                Some(tool_name.to_string()),
                actor,
                config.clone(),
            ).await?;
            actor_ref
        }
        "write" => {
            let actor = WriteActor::new(config.clone());
            let (actor_ref, _) = Actor::spawn(
                Some(tool_name.to_string()),
                actor,
                config.clone(),
            ).await?;
            actor_ref
        }
        "edit" => {
            let actor = EditActor::new(config.clone());
            let (actor_ref, _) = Actor::spawn(
                Some(tool_name.to_string()),
                actor,
                config.clone(),
            ).await?;
            actor_ref
        }
        "glob" => {
            let actor = GlobActor::new(config.clone());
            let (actor_ref, _) = Actor::spawn(
                Some(tool_name.to_string()),
                actor,
                config.clone(),
            ).await?;
            actor_ref
        }
        "grep" => {
            let actor = GrepActor::new(config.clone());
            let (actor_ref, _) = Actor::spawn(
                Some(tool_name.to_string()),
                actor,
                config.clone(),
            ).await?;
            actor_ref
        }
        "read_many_files" => {
            let actor = ReadManyFilesActor::new(config.clone());
            let (actor_ref, _) = Actor::spawn(
                Some(tool_name.to_string()),
                actor,
                config.clone(),
            ).await?;
            actor_ref
        }
        "bash" => {
            let actor = BashActor::new(config.clone());
            let (actor_ref, _) = Actor::spawn(
                Some(tool_name.to_string()),
                actor,
                config.clone(),
            ).await?;
            actor_ref
        }
        "web_search" => {
            let actor = WebSearchActor::new(config.clone());
            let (actor_ref, _) = Actor::spawn(
                Some(tool_name.to_string()),
                actor,
                config.clone(),
            ).await?;
            actor_ref
        }
        "web_fetch" => {
            let actor = WebFetchActor::new(config.clone());
            let (actor_ref, _) = Actor::spawn(
                Some(tool_name.to_string()),
                actor,
                config.clone(),
            ).await?;
            actor_ref
        }
        "memory" => {
            let actor = MemoryActor::new(config.clone()).await?;
            let (actor_ref, _) = Actor::spawn(
                Some(tool_name.to_string()),
                actor,
                config.clone(),
            ).await?;
            actor_ref
        }
        "todo" => {
            let actor = TodoActor::new(config.clone()).await?;
            let (actor_ref, _) = Actor::spawn(
                Some(tool_name.to_string()),
                actor,
                config.clone(),
            ).await?;
            actor_ref
        }
        "knowledge_agent" => {
            let actor = knowledge_agent::KnowledgeAgentActor::new(config.clone()).await?;
            let (actor_ref, _) = Actor::spawn(
                Some(tool_name.to_string()),
                actor,
                config.clone(),
            ).await?;
            actor_ref
        }
        _ => {
            return Err(anyhow!("Unknown tool: {}", tool_name));
        }
    };
    
    // Execute the tool
    let id = Uuid::new_v4();
    tool_ref.send_message(ToolMessage::Execute {
        id,
        params,
        chat_ref,
    })?;
    
    // Wait for the result
    match tokio::time::timeout(tokio::time::Duration::from_secs(30), rx.recv()).await {
        Ok(Some(result)) => Ok(result),
        Ok(None) => Err(anyhow!("Tool did not return a result")),
        Err(_) => Err(anyhow!("Tool execution timed out after 30 seconds")),
    }
}