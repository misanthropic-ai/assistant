use crate::{
    events::{handle_key_event, Action},
    state::{AppState, MessageType},
    ui::render,
    display_actor::DisplayActor,
};
use anyhow::Result;
use assistant_core::{
    config::Config,
    messages::{ChatMessage, UserMessageContent, DisplayContext},
    ractor::{Actor, ActorRef},
    actors::conversation_manager::{ConversationManagerActor, ConversationManagerMessage},
    persistence::database::Database,
};
use crossterm::{
    event::{Event, EventStream},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use futures::{StreamExt, FutureExt};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::{io::{self, IsTerminal}, time::Duration};
use tokio::sync::mpsc;
use uuid::Uuid;

pub struct TuiApp {
    state: AppState,
    terminal: Terminal<CrosstermBackend<io::Stdout>>,
    actor_system: Option<assistant_core::actor_init::ActorSystem>,
    response_rx: mpsc::UnboundedReceiver<ChatMessage>,
    conversation_manager: Option<ActorRef<ConversationManagerMessage>>,
}

impl TuiApp {
    pub async fn new(config: Config) -> Result<Self> {
        // Check if we're in a TTY
        if !io::stdout().is_terminal() {
            return Err(anyhow::anyhow!("The interactive TUI requires a terminal. Please run this command in a terminal emulator."));
        }
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen)?;
        
        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend)?;
        
        // Clear the terminal
        terminal.clear()?;
        
        let mut state = AppState::new(config.clone());
        
        // Update terminal size
        let size = terminal.size()?;
        state.update_terminal_size(size.width, size.height);
        
        let (response_tx, response_rx) = mpsc::unbounded_channel();
        
        // Initialize database for conversation manager
        let db_path = config.session.database_path.clone()
            .unwrap_or_else(|| {
                dirs::home_dir()
                    .unwrap_or_else(|| std::path::PathBuf::from("/tmp"))
                    .join(".assistant")
                    .join("assistant.db")
            });
        
        let database = Database::new(&db_path).await?;
        
        // Create conversation manager
        let conversation_manager_actor = ConversationManagerActor::new(database).await?;
        let (conv_manager_ref, _) = Actor::spawn(
            Some("conversation_manager".to_string()),
            conversation_manager_actor,
            (),
        )
        .await?;
        
        // Initialize the actor system
        let actor_system = match assistant_core::actor_init::init_actor_system(config.clone()).await {
            Ok(actors) => {
                // Create and spawn the TUI display actor
                let display_actor = DisplayActor::new(response_tx.clone());
                let (display_ref, _) = Actor::spawn(
                    Some("tui_display".to_string()),
                    display_actor,
                    response_tx,
                )
                .await?;
                
                // Register the display actor with the chat system
                actors.chat.send_message(ChatMessage::RegisterDisplay {
                    context: DisplayContext::TUI,
                    display_ref,
                })?;
                
                Some(actors)
            }
            Err(e) => {
                tracing::error!("Failed to initialize actor system: {}", e);
                state.add_message(
                    MessageType::Error,
                    format!("Failed to initialize chat system: {}", e),
                );
                None
            }
        };
        
        Ok(Self {
            state,
            terminal,
            actor_system,
            response_rx,
            conversation_manager: Some(conv_manager_ref),
        })
    }
    
    pub async fn run(&mut self) -> Result<()> {
        let mut event_stream = EventStream::new();
        
        loop {
            // Draw the UI
            self.terminal.draw(|frame| render(frame, &self.state))?;
            
            // Use tokio::select! to handle multiple event sources
            tokio::select! {
                // Terminal events (keyboard, resize)
                maybe_event = event_stream.next().fuse() => {
                    match maybe_event {
                        Some(Ok(Event::Key(key))) => {
                            if let Some(action) = handle_key_event(key) {
                                self.handle_action(action).await?;
                            }
                        }
                        Some(Ok(Event::Resize(width, height))) => {
                            self.state.update_terminal_size(width, height);
                        }
                        Some(Err(e)) => {
                            tracing::error!("Error reading event: {}", e);
                        }
                        None => break,
                        _ => {}
                    }
                }
                
                // Chat messages from actors
                Some(msg) = self.response_rx.recv() => {
                    self.handle_chat_message(msg);
                }
                
                // Add periodic refresh
                _ = tokio::time::sleep(Duration::from_millis(100)).fuse() => {
                    // This ensures we don't block forever
                }
            }
            
            if self.state.should_quit {
                break;
            }
        }
        
        Ok(())
    }
    
    async fn handle_action(&mut self, action: Action) -> Result<()> {
        // Handle actions based on current view mode
        match &self.state.view_mode {
            crate::state::ViewMode::Chat => self.handle_chat_action(action).await,
            crate::state::ViewMode::ConversationList => self.handle_conversation_list_action(action).await,
            crate::state::ViewMode::RenameDialog { .. } => self.handle_rename_dialog_action(action).await,
            crate::state::ViewMode::DeleteConfirmation { .. } => self.handle_delete_confirmation_action(action).await,
        }
    }
    
    async fn handle_chat_action(&mut self, action: Action) -> Result<()> {
        match action {
            Action::Quit => {
                self.state.should_quit = true;
            }
            Action::ClearScreen => {
                self.state.messages.clear();
                self.state.scroll_to_bottom();
            }
            Action::Submit => {
                let input = self.state.input.submit();
                if !input.trim().is_empty() {
                    self.state.add_message(MessageType::User, input.clone());
                    self.send_to_assistant(input).await;
                }
            }
            Action::InsertChar(c) => {
                self.state.input.insert_char(c);
            }
            Action::DeleteChar => {
                self.state.input.delete_char();
            }
            Action::CursorLeft => {
                self.state.input.move_cursor_left();
            }
            Action::CursorRight => {
                self.state.input.move_cursor_right();
            }
            Action::CursorHome => {
                self.state.input.move_cursor_home();
            }
            Action::CursorEnd => {
                self.state.input.move_cursor_end();
            }
            Action::HistoryPrevious => {
                self.state.input.history_previous();
            }
            Action::HistoryNext => {
                self.state.input.history_next();
            }
            Action::ScrollUp => {
                self.state.scroll_up(1);
            }
            Action::ScrollDown => {
                self.state.scroll_down(1);
            }
            Action::ToggleConversationList => {
                // Load conversations and switch to list view
                self.load_conversations().await;
                self.state.view_mode = crate::state::ViewMode::ConversationList;
            }
            _ => {}
        }
        Ok(())
    }
    
    async fn handle_conversation_list_action(&mut self, action: Action) -> Result<()> {
        match action {
            Action::Quit => {
                self.state.should_quit = true;
            }
            Action::Escape | Action::ToggleConversationList => {
                // Return to chat view
                self.state.view_mode = crate::state::ViewMode::Chat;
            }
            Action::Submit => {
                // Load selected conversation
                let selected_id = self.state.conversation_list.conversations
                    .get(self.state.conversation_list.selected_index)
                    .map(|s| s.id.clone());
                
                if let Some(session_id) = selected_id {
                    self.switch_to_conversation(&session_id).await;
                    self.state.view_mode = crate::state::ViewMode::Chat;
                }
            }
            Action::HistoryPrevious | Action::ScrollUp => {
                // Move selection up
                if self.state.conversation_list.selected_index > 0 {
                    self.state.conversation_list.selected_index -= 1;
                }
            }
            Action::HistoryNext | Action::ScrollDown => {
                // Move selection down
                let max_index = self.state.conversation_list.conversations.len().saturating_sub(1);
                if self.state.conversation_list.selected_index < max_index {
                    self.state.conversation_list.selected_index += 1;
                }
            }
            Action::Delete => {
                // Show delete confirmation
                if let Some(selected) = self.state.conversation_list.conversations
                    .get(self.state.conversation_list.selected_index)
                {
                    self.state.view_mode = crate::state::ViewMode::DeleteConfirmation {
                        session_id: selected.id.clone(),
                    };
                }
            }
            Action::InsertChar('n') => {
                // Create new conversation
                self.create_new_conversation().await;
            }
            Action::InsertChar('r') => {
                // Show rename dialog
                if let Some(selected) = self.state.conversation_list.conversations
                    .get(self.state.conversation_list.selected_index)
                {
                    self.state.view_mode = crate::state::ViewMode::RenameDialog {
                        session_id: selected.id.clone(),
                    };
                }
            }
            Action::InsertChar('/') => {
                // Start search
                self.state.conversation_list.is_searching = true;
                self.state.conversation_list.search_query.clear();
            }
            Action::InsertChar('d') => {
                // Also handle 'd' for delete
                if let Some(selected) = self.state.conversation_list.conversations
                    .get(self.state.conversation_list.selected_index)
                {
                    self.state.view_mode = crate::state::ViewMode::DeleteConfirmation {
                        session_id: selected.id.clone(),
                    };
                }
            }
            _ => {}
        }
        Ok(())
    }
    
    async fn handle_rename_dialog_action(&mut self, action: Action) -> Result<()> {
        match action {
            Action::Escape => {
                // Cancel rename and return to list
                self.state.view_mode = crate::state::ViewMode::ConversationList;
            }
            // TODO: Implement rename dialog input handling
            _ => {}
        }
        Ok(())
    }
    
    async fn handle_delete_confirmation_action(&mut self, action: Action) -> Result<()> {
        match action {
            Action::Escape | Action::InsertChar('n') | Action::InsertChar('N') => {
                // Cancel delete and return to list
                self.state.view_mode = crate::state::ViewMode::ConversationList;
            }
            Action::InsertChar('y') | Action::InsertChar('Y') => {
                // Confirm delete
                if let crate::state::ViewMode::DeleteConfirmation { session_id } = &self.state.view_mode.clone() {
                    self.delete_conversation(session_id).await;
                    self.state.view_mode = crate::state::ViewMode::ConversationList;
                    // Reload conversations
                    self.load_conversations().await;
                }
            }
            _ => {}
        }
        Ok(())
    }
    
    async fn send_to_assistant(&mut self, input: String) {
        if let Some(ref actor_system) = self.actor_system {
            // Create a new session if needed (only when actually sending a message)
            if self.state.current_session_id.is_none() {
                if let Some(ref conv_manager) = self.conversation_manager {
                    let (tx, rx) = tokio::sync::oneshot::channel();
                    
                    if let Err(e) = conv_manager.send_message(ConversationManagerMessage::CreateConversation {
                        reply_to: tx,
                        workspace_path: None,
                    }) {
                        self.state.add_message(
                            MessageType::Error,
                            format!("Failed to create conversation: {}", e),
                        );
                        return;
                    }
                    
                    match rx.await {
                        Ok(Ok(session_id)) => {
                            self.state.current_session_id = Some(session_id.clone());
                            
                            // Update the chat actor to use the new session
                            if let Err(e) = conv_manager.send_message(ConversationManagerMessage::SwitchConversation {
                                session_id,
                                chat_ref: actor_system.chat.clone(),
                            }) {
                                self.state.add_message(
                                    MessageType::Error,
                                    format!("Failed to switch to new conversation: {}", e),
                                );
                                return;
                            }
                        }
                        _ => {
                            self.state.add_message(
                                MessageType::Error,
                                "Failed to create conversation".to_string(),
                            );
                            return;
                        }
                    }
                }
            }
            
            let message_id = self.state.start_streaming_message(MessageType::Assistant);
            
            if let Err(e) = actor_system.chat.send_message(ChatMessage::UserPrompt { 
                id: Uuid::new_v4(),
                content: UserMessageContent::Text(input),
                context: DisplayContext::TUI,
            }) {
                self.state.finish_streaming_message(message_id);
                self.state.add_message(
                    MessageType::Error, 
                    format!("Failed to send message: {}", e)
                );
            }
        } else {
            self.state.add_message(
                MessageType::Error,
                "Chat system not initialized".to_string()
            );
        }
    }
    
    fn handle_chat_message(&mut self, msg: ChatMessage) {
        match msg {
            ChatMessage::StreamToken { token } => {
                if let Some(msg) = self.state.messages.iter_mut().rev().find(|m| m.is_streaming) {
                    msg.content.push_str(&token);
                    self.state.scroll_to_bottom();
                }
            }
            ChatMessage::Complete { id: _, response } => {
                if let Some(msg) = self.state.messages.iter_mut().rev().find(|m| m.is_streaming) {
                    msg.content = response;
                    msg.is_streaming = false;
                    self.state.is_streaming = false;
                }
            }
            ChatMessage::Error { id: _, error } => {
                self.state.add_message(MessageType::Error, error);
            }
            ChatMessage::ToolRequest { id: _, call } => {
                self.state.add_message(
                    MessageType::Tool { name: call.tool_name.clone() }, 
                    format!("Calling tool: {}", call.tool_name)
                );
            }
            ChatMessage::ToolResult { id: _, result } => {
                self.state.add_message(MessageType::Info, result);
            }
            _ => {}
        }
    }
    
    async fn load_conversations(&mut self) {
        if let Some(ref conv_manager) = self.conversation_manager {
            let (tx, rx) = tokio::sync::oneshot::channel();
            
            if let Err(e) = conv_manager.send_message(ConversationManagerMessage::ListConversations {
                reply_to: tx,
                limit: 50,
                offset: 0,
            }) {
                self.state.add_message(
                    MessageType::Error,
                    format!("Failed to load conversations: {}", e),
                );
                return;
            }
            
            match rx.await {
                Ok(Ok(conversations)) => {
                    self.state.conversation_list.conversations = conversations;
                    self.state.conversation_list.selected_index = 0;
                }
                Ok(Err(e)) => {
                    self.state.add_message(
                        MessageType::Error,
                        format!("Failed to load conversations: {}", e),
                    );
                }
                Err(e) => {
                    self.state.add_message(
                        MessageType::Error,
                        format!("Failed to receive conversations: {}", e),
                    );
                }
            }
        }
    }
    
    async fn switch_to_conversation(&mut self, session_id: &str) {
        tracing::info!("Switching to conversation: {}", session_id);
        
        if let (Some(conv_manager), Some(actor_system)) = (&self.conversation_manager, &self.actor_system) {
            // First load the conversation messages
            let (tx, rx) = tokio::sync::oneshot::channel();
            
            if let Err(e) = conv_manager.send_message(ConversationManagerMessage::LoadConversation {
                reply_to: tx,
                session_id: session_id.to_string(),
            }) {
                self.state.add_message(
                    MessageType::Error,
                    format!("Failed to load conversation: {}", e),
                );
                return;
            }
            
            match rx.await {
                Ok(Ok(messages)) => {
                    tracing::info!("Received {} messages for session {}", messages.len(), session_id);
                    
                    // Clear current messages and load the conversation
                    self.state.messages.clear();
                    self.state.current_session_id = Some(session_id.to_string());
                    
                    // Convert OpenAI messages to UI messages
                    for (idx, msg) in messages.iter().enumerate() {
                        match msg {
                            assistant_core::openai_compat::ChatMessage::User { content, .. } => {
                                let text = match content {
                                    assistant_core::openai_compat::UserContent::Text(t) => t.clone(),
                                    assistant_core::openai_compat::UserContent::Array(parts) => {
                                        parts.iter()
                                            .filter_map(|p| match p {
                                                assistant_core::openai_compat::ContentPart::Text { text } => Some(text.clone()),
                                                assistant_core::openai_compat::ContentPart::Image { .. } => Some("[Image]".to_string()),
                                            })
                                            .collect::<Vec<_>>()
                                            .join(" ")
                                    }
                                };
                                tracing::info!("Message {}: User - {}", idx, text);
                                self.state.add_message(MessageType::User, text);
                            }
                            assistant_core::openai_compat::ChatMessage::Assistant { content, .. } => {
                                if let Some(text) = content {
                                    tracing::info!("Message {}: Assistant - {}", idx, text);
                                    self.state.add_message(MessageType::Assistant, text.clone());
                                } else {
                                    tracing::info!("Message {}: Assistant with no content", idx);
                                }
                            }
                            assistant_core::openai_compat::ChatMessage::System { .. } => {
                                tracing::info!("Message {}: System (skipped)", idx);
                            }
                            assistant_core::openai_compat::ChatMessage::Tool { .. } => {
                                tracing::info!("Message {}: Tool (skipped)", idx);
                            }
                        }
                    }
                    
                    tracing::info!("After conversion, UI has {} messages", self.state.messages.len());
                    
                    // Switch the chat actor to this conversation
                    if let Err(e) = conv_manager.send_message(ConversationManagerMessage::SwitchConversation {
                        session_id: session_id.to_string(),
                        chat_ref: actor_system.chat.clone(),
                    }) {
                        self.state.add_message(
                            MessageType::Error,
                            format!("Failed to switch conversation: {}", e),
                        );
                    }
                }
                Ok(Err(e)) => {
                    self.state.add_message(
                        MessageType::Error,
                        format!("Failed to load conversation: {}", e),
                    );
                }
                Err(e) => {
                    self.state.add_message(
                        MessageType::Error,
                        format!("Failed to receive conversation: {}", e),
                    );
                }
            }
        }
    }
    
    async fn create_new_conversation(&mut self) {
        if let Some(ref conv_manager) = self.conversation_manager {
            let (tx, rx) = tokio::sync::oneshot::channel();
            
            if let Err(e) = conv_manager.send_message(ConversationManagerMessage::CreateConversation {
                reply_to: tx,
                workspace_path: None,
            }) {
                self.state.add_message(
                    MessageType::Error,
                    format!("Failed to create conversation: {}", e),
                );
                return;
            }
            
            match rx.await {
                Ok(Ok(session_id)) => {
                    // Clear messages and switch to new conversation
                    self.state.messages.clear();
                    self.state.current_session_id = Some(session_id.clone());
                    self.state.add_message(
                        MessageType::Info,
                        "New conversation started. Type your message and press Enter to chat.".to_string(),
                    );
                    
                    // Switch back to chat view
                    self.state.view_mode = crate::state::ViewMode::Chat;
                    
                    // Update the chat actor to use the new session
                    if let Some(ref actor_system) = self.actor_system {
                        if let Err(e) = conv_manager.send_message(ConversationManagerMessage::SwitchConversation {
                            session_id,
                            chat_ref: actor_system.chat.clone(),
                        }) {
                            self.state.add_message(
                                MessageType::Error,
                                format!("Failed to switch to new conversation: {}", e),
                            );
                        }
                    }
                }
                Ok(Err(e)) => {
                    self.state.add_message(
                        MessageType::Error,
                        format!("Failed to create conversation: {}", e),
                    );
                }
                Err(e) => {
                    self.state.add_message(
                        MessageType::Error,
                        format!("Failed to receive new conversation ID: {}", e),
                    );
                }
            }
        }
    }
    
    async fn delete_conversation(&mut self, session_id: &str) {
        if let Some(ref conv_manager) = self.conversation_manager {
            let (tx, rx) = tokio::sync::oneshot::channel();
            
            if let Err(e) = conv_manager.send_message(ConversationManagerMessage::DeleteConversation {
                reply_to: tx,
                session_id: session_id.to_string(),
            }) {
                self.state.add_message(
                    MessageType::Error,
                    format!("Failed to delete conversation: {}", e),
                );
                return;
            }
            
            match rx.await {
                Ok(Ok(())) => {
                    // If we deleted the current conversation, create a new one
                    if self.state.current_session_id.as_ref() == Some(&session_id.to_string()) {
                        self.create_new_conversation().await;
                    }
                }
                Ok(Err(e)) => {
                    self.state.add_message(
                        MessageType::Error,
                        format!("Failed to delete conversation: {}", e),
                    );
                }
                Err(e) => {
                    self.state.add_message(
                        MessageType::Error,
                        format!("Failed to receive delete confirmation: {}", e),
                    );
                }
            }
        }
    }
}

impl Drop for TuiApp {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(self.terminal.backend_mut(), LeaveAlternateScreen);
    }
}