use crate::{
    events::{handle_key_event, Action},
    state::{AppState, MessageType},
    ui::render,
};
use anyhow::Result;
use assistant_core::{
    config::Config,
    messages::{ChatMessage, UserMessageContent, DisplayContext},
    ractor::ActorRef,
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
    chat_ref: Option<ActorRef<ChatMessage>>,
    response_rx: mpsc::UnboundedReceiver<ChatMessage>,
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
        
        let (_response_tx, response_rx) = mpsc::unbounded_channel();
        
        // For now, we'll initialize without the full actor system
        // In a real implementation, we'd use the actor_init module
        let chat_ref = None;
        
        Ok(Self {
            state,
            terminal,
            chat_ref,
            response_rx,
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
            _ => {}
        }
        Ok(())
    }
    
    async fn send_to_assistant(&mut self, input: String) {
        if let Some(ref chat_ref) = self.chat_ref {
            let message_id = self.state.start_streaming_message(MessageType::Assistant);
            
            if let Err(e) = chat_ref.send_message(ChatMessage::UserPrompt { 
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
}

impl Drop for TuiApp {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(self.terminal.backend_mut(), LeaveAlternateScreen);
    }
}