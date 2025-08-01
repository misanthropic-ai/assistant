use anyhow::{Result, anyhow};
use assistant_core::{
    actors::{tools::*, delegator::DelegatorActor},
    config::Config,
    messages::{ToolMessage, ChatMessage, DelegatorMessage, ToolCall},
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

    // Determine if this tool should be delegated to a sub-agent (vision-enabled, etc.)
    let delegate_tool = config
        .get_tool_config(tool_name)
        .map(|tc| tc.should_delegate())
        .unwrap_or(false);

    // Create a channel to receive results
    let (tx, mut rx) = mpsc::unbounded_channel();

    // Create the simple chat actor that will forward tool results through the channel
    let chat_actor = SimpleChatActor { sender: tx.clone() };
    let (chat_ref, _) = Actor::spawn(
        Some("simple-chat".to_string()),
        chat_actor,
        tx,
    )
    .await?;

    // Unique ID for this request (used for correlation across actors)
    let id = Uuid::new_v4();

    if delegate_tool {
        // ------------------------------------------------------------------
        // Use the DelegatorActor so the request is handled by the configured
        // sub-agent (e.g. vision model for `computer_use`).
        // ------------------------------------------------------------------
        let delegator = DelegatorActor::new(config.clone());
        let (delegator_ref, _) = Actor::spawn(
            Some("delegator".to_string()),
            delegator,
            config.clone(),
        )
        .await?;

        // Build the ToolCall to route through the delegator
        let call = ToolCall {
            tool_name: tool_name.to_string(),
            parameters: params.clone(),
            delegate: true,
        };

        delegator_ref.send_message(DelegatorMessage::RouteToolCall {
            id,
            call,
            chat_ref: chat_ref.clone(),
        })?;
    } else {
        // ------------------------------------------------------------------
        // Local execution path (existing behaviour for non-delegated tools)
        // ------------------------------------------------------------------
        let tool_ref: ActorRef<ToolMessage> = match tool_name {
            "ls" => {
                let actor = LsActor::new(config.clone());
                let (actor_ref, _) = Actor::spawn(
                    Some(tool_name.to_string()),
                    actor,
                    config.clone(),
                )
                .await?;
                actor_ref
            }
            "read" => {
                let actor = ReadActor::new(config.clone());
                let (actor_ref, _) = Actor::spawn(
                    Some(tool_name.to_string()),
                    actor,
                    config.clone(),
                )
                .await?;
                actor_ref
            }
            "write" => {
                let actor = WriteActor::new(config.clone());
                let (actor_ref, _) = Actor::spawn(
                    Some(tool_name.to_string()),
                    actor,
                    config.clone(),
                )
                .await?;
                actor_ref
            }
            "edit" => {
                let actor = EditActor::new(config.clone());
                let (actor_ref, _) = Actor::spawn(
                    Some(tool_name.to_string()),
                    actor,
                    config.clone(),
                )
                .await?;
                actor_ref
            }
            "glob" => {
                let actor = GlobActor::new(config.clone());
                let (actor_ref, _) = Actor::spawn(
                    Some(tool_name.to_string()),
                    actor,
                    config.clone(),
                )
                .await?;
                actor_ref
            }
            "grep" => {
                let actor = GrepActor::new(config.clone());
                let (actor_ref, _) = Actor::spawn(
                    Some(tool_name.to_string()),
                    actor,
                    config.clone(),
                )
                .await?;
                actor_ref
            }
            "read_many_files" => {
                let actor = ReadManyFilesActor::new(config.clone());
                let (actor_ref, _) = Actor::spawn(
                    Some(tool_name.to_string()),
                    actor,
                    config.clone(),
                )
                .await?;
                actor_ref
            }
            "bash" => {
                let actor = BashActor::new(config.clone());
                let (actor_ref, _) = Actor::spawn(
                    Some(tool_name.to_string()),
                    actor,
                    config.clone(),
                )
                .await?;
                actor_ref
            }
            "web_search" => {
                let actor = WebSearchActor::new(config.clone());
                let (actor_ref, _) = Actor::spawn(
                    Some(tool_name.to_string()),
                    actor,
                    config.clone(),
                )
                .await?;
                actor_ref
            }
            "web_fetch" => {
                let actor = WebFetchActor::new(config.clone());
                let (actor_ref, _) = Actor::spawn(
                    Some(tool_name.to_string()),
                    actor,
                    config.clone(),
                )
                .await?;
                actor_ref
            }
            "memory" => {
                let actor = MemoryActor::new(config.clone()).await?;
                let (actor_ref, _) = Actor::spawn(
                    Some(tool_name.to_string()),
                    actor,
                    config.clone(),
                )
                .await?;
                actor_ref
            }
            "todo" => {
                let actor = TodoActor::new(config.clone()).await?;
                let (actor_ref, _) = Actor::spawn(
                    Some(tool_name.to_string()),
                    actor,
                    config.clone(),
                )
                .await?;
                actor_ref
            }
            "knowledge_agent" => {
                let actor = knowledge_agent::KnowledgeAgentActor::new(config.clone()).await?;
                let (actor_ref, _) = Actor::spawn(
                    Some(tool_name.to_string()),
                    actor,
                    config.clone(),
                )
                .await?;
                actor_ref
            }
            "screenshot" => {
                let actor = ScreenshotActor::new(config.clone());
                let (actor_ref, _) = Actor::spawn(
                    Some(tool_name.to_string()),
                    actor,
                    config.clone(),
                )
                .await?;
                actor_ref
            }
            "desktop_control" => {
                let actor = DesktopControlActor::new(config.clone());
                let (actor_ref, _) = Actor::spawn(
                    Some(tool_name.to_string()),
                    actor,
                    config.clone(),
                )
                .await?;
                actor_ref
            }
            "computer_use" => {
                let actor = ComputerUseActor::new(config.clone());
                let (actor_ref, _) = Actor::spawn(
                    Some(tool_name.to_string()),
                    actor,
                    config.clone(),
                )
                .await?;
                actor_ref
            }
            _ => {
                return Err(anyhow!("Unknown tool: {}", tool_name));
            }
        };

        // Execute the tool locally
        tool_ref.send_message(ToolMessage::Execute {
            id,
            params,
            chat_ref: chat_ref.clone(),
        })?;
    }

    // ----------------------------------------------------------------------
    // Wait for the result from either the delegator path or the local path.
    // ----------------------------------------------------------------------
    match tokio::time::timeout(tokio::time::Duration::from_secs(60), rx.recv()).await {
        Ok(Some(result)) => Ok(result),
        Ok(None) => Err(anyhow!("Tool did not return a result")),
        Err(_) => Err(anyhow!("Tool execution timed out after 60 seconds")),
    }
}