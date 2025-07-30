use anyhow::Result;
use assistant_core::{
    actors::{
        supervisor::SupervisorActor,
        chat::ChatActor,
        client::{ClientActor, ClientMessage},
        delegator::DelegatorActor,
        tools::{
            ls::LsActor,
            read::ReadActor,
            write::WriteActor,
            edit::EditActor,
            glob::GlobActor,
            grep::GrepActor,
            bash::BashActor,
            web_search::WebSearchActor,
            web_fetch::WebFetchActor,
            memory::MemoryActor,
            todo::TodoActor,
            read_many_files::ReadManyFilesActor,
        },
    },
    config::Config,
    messages::{ChatMessage, DelegatorMessage, ToolMessage},
    ractor::{Actor, ActorRef},
};
use std::sync::Arc;

/// All the actor references needed for the system
pub struct ActorSystem {
    pub supervisor: ActorRef<assistant_core::messages::SupervisorMessage>,
    pub chat: ActorRef<ChatMessage>,
    pub client: ActorRef<ClientMessage>,
    pub delegator: ActorRef<DelegatorMessage>,
}

/// Initialize all actors and wire them together
pub async fn init_actor_system(config: Config) -> Result<ActorSystem> {
    tracing::info!("Initializing actor system");
    
    let config = Arc::new(config);
    
    // Create supervisor
    let supervisor = SupervisorActor::new(config.as_ref().clone());
    let (supervisor_ref, _) = Actor::spawn(
        Some("supervisor".to_string()),
        supervisor,
        config.as_ref().clone(),
    )
    .await?;
    
    // Create delegator
    let delegator = DelegatorActor::new(config.as_ref().clone());
    let (delegator_ref, _) = Actor::spawn(
        Some("delegator".to_string()),
        delegator,
        config.as_ref().clone(),
    )
    .await?;
    
    // Create client
    let client = ClientActor::new(config.as_ref().clone());
    let (client_ref, _) = Actor::spawn(
        Some("client".to_string()),
        client,
        config.as_ref().clone(),
    )
    .await?;
    
    // Create chat with references
    let chat = ChatActor::new(config.as_ref().clone())
        .with_client_ref(client_ref.clone())
        .with_delegator_ref(delegator_ref.clone());
    let (chat_ref, _) = Actor::spawn(
        Some("chat".to_string()),
        chat,
        config.as_ref().clone(),
    )
    .await?;
    
    // Set chat ref on client
    client_ref.send_message(ClientMessage::SetChatRef(chat_ref.clone()))?;
    
    // Set delegator ref on chat
    chat_ref.send_message(ChatMessage::SetDelegatorRef(delegator_ref.clone()))?;
    
    // Register all tool actors with delegator
    register_tools(&delegator_ref, &config).await?;
    
    Ok(ActorSystem {
        supervisor: supervisor_ref,
        chat: chat_ref,
        client: client_ref,
        delegator: delegator_ref,
    })
}

/// Register all tool actors with the delegator
async fn register_tools(delegator_ref: &ActorRef<DelegatorMessage>, config: &Config) -> Result<()> {
    // Helper to check if tool is enabled
    let is_enabled = |name: &str| -> bool {
        !config.tools.exclude.contains(&name.to_string()) &&
        config.tools.configs.get(name).map(|tc| tc.enabled).unwrap_or(true)
    };
    
    // Register ls tool
    if is_enabled("ls") {
        let (ls_ref, _): (ActorRef<ToolMessage>, _) = Actor::spawn(
            Some("tool_ls".to_string()),
            LsActor::new(config.clone()),
            config.clone(),
        )
        .await?;
        delegator_ref.send_message(DelegatorMessage::RegisterTool {
            name: "ls".to_string(),
            actor_ref: ls_ref,
        })?;
    }
    
    // Register read tool
    if is_enabled("read") {
        let (read_ref, _): (ActorRef<ToolMessage>, _) = Actor::spawn(
            Some("tool_read".to_string()),
            ReadActor::new(config.clone()),
            config.clone(),
        )
        .await?;
        delegator_ref.send_message(DelegatorMessage::RegisterTool {
            name: "read".to_string(),
            actor_ref: read_ref,
        })?;
    }
    
    // Register write tool
    if is_enabled("write") {
        let (write_ref, _): (ActorRef<ToolMessage>, _) = Actor::spawn(
            Some("tool_write".to_string()),
            WriteActor::new(config.clone()),
            config.clone(),
        )
        .await?;
        delegator_ref.send_message(DelegatorMessage::RegisterTool {
            name: "write".to_string(),
            actor_ref: write_ref,
        })?;
    }
    
    // Register edit tool
    if is_enabled("edit") {
        let (edit_ref, _): (ActorRef<ToolMessage>, _) = Actor::spawn(
            Some("tool_edit".to_string()),
            EditActor::new(config.clone()),
            config.clone(),
        )
        .await?;
        delegator_ref.send_message(DelegatorMessage::RegisterTool {
            name: "edit".to_string(),
            actor_ref: edit_ref,
        })?;
    }
    
    // Register glob tool
    if is_enabled("glob") {
        let (glob_ref, _): (ActorRef<ToolMessage>, _) = Actor::spawn(
            Some("tool_glob".to_string()),
            GlobActor::new(config.clone()),
            config.clone(),
        )
        .await?;
        delegator_ref.send_message(DelegatorMessage::RegisterTool {
            name: "glob".to_string(),
            actor_ref: glob_ref,
        })?;
    }
    
    // Register grep tool
    if is_enabled("grep") {
        let (grep_ref, _): (ActorRef<ToolMessage>, _) = Actor::spawn(
            Some("tool_grep".to_string()),
            GrepActor::new(config.clone()),
            config.clone(),
        )
        .await?;
        delegator_ref.send_message(DelegatorMessage::RegisterTool {
            name: "grep".to_string(),
            actor_ref: grep_ref,
        })?;
    }
    
    // Register bash tool
    if is_enabled("bash") {
        let (bash_ref, _): (ActorRef<ToolMessage>, _) = Actor::spawn(
            Some("tool_bash".to_string()),
            BashActor::new(config.clone()),
            config.clone(),
        )
        .await?;
        delegator_ref.send_message(DelegatorMessage::RegisterTool {
            name: "bash".to_string(),
            actor_ref: bash_ref,
        })?;
    }
    
    // Register web_search tool
    if is_enabled("web_search") {
        let (web_search_ref, _): (ActorRef<ToolMessage>, _) = Actor::spawn(
            Some("tool_web_search".to_string()),
            WebSearchActor::new(config.clone()),
            config.clone(),
        )
        .await?;
        delegator_ref.send_message(DelegatorMessage::RegisterTool {
            name: "web_search".to_string(),
            actor_ref: web_search_ref,
        })?;
    }
    
    // Register web_fetch tool
    if is_enabled("web_fetch") {
        let (web_fetch_ref, _): (ActorRef<ToolMessage>, _) = Actor::spawn(
            Some("tool_web_fetch".to_string()),
            WebFetchActor::new(config.clone()),
            config.clone(),
        )
        .await?;
        delegator_ref.send_message(DelegatorMessage::RegisterTool {
            name: "web_fetch".to_string(),
            actor_ref: web_fetch_ref,
        })?;
    }
    
    // Register memory tool
    if is_enabled("memory") {
        let memory_actor = MemoryActor::new(config.clone()).await?;
        let (memory_ref, _): (ActorRef<ToolMessage>, _) = Actor::spawn(
            Some("tool_memory".to_string()),
            memory_actor,
            config.clone(),
        )
        .await?;
        delegator_ref.send_message(DelegatorMessage::RegisterTool {
            name: "memory".to_string(),
            actor_ref: memory_ref,
        })?;
    }
    
    // Register todo tool
    if is_enabled("todo") {
        let (todo_ref, _): (ActorRef<ToolMessage>, _) = Actor::spawn(
            Some("tool_todo".to_string()),
            TodoActor::new(config.clone()),
            config.clone(),
        )
        .await?;
        delegator_ref.send_message(DelegatorMessage::RegisterTool {
            name: "todo".to_string(),
            actor_ref: todo_ref,
        })?;
    }
    
    // Register read_many_files tool
    if is_enabled("read_many_files") {
        let (read_many_ref, _): (ActorRef<ToolMessage>, _) = Actor::spawn(
            Some("tool_read_many_files".to_string()),
            ReadManyFilesActor::new(config.clone()),
            config.clone(),
        )
        .await?;
        delegator_ref.send_message(DelegatorMessage::RegisterTool {
            name: "read_many_files".to_string(),
            actor_ref: read_many_ref,
        })?;
    }
    
    tracing::info!("All tools registered with delegator");
    Ok(())
}