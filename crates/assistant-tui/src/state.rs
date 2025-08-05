use assistant_core::Config;
use chrono::{DateTime, Utc};
use std::collections::VecDeque;

#[derive(Debug, Clone, PartialEq)]
pub enum MessageType {
    User,
    Assistant,
    Tool { name: String },
    Error,
    Info,
    #[allow(dead_code)]
    System,
}

#[derive(Debug, Clone)]
pub struct Message {
    pub id: usize,
    pub message_type: MessageType,
    pub content: String,
    pub timestamp: DateTime<Utc>,
    pub is_streaming: bool,
}

#[derive(Debug, Clone, PartialEq)]
#[allow(dead_code)]
pub enum AppMode {
    Normal,
    Insert,
    Command,
}

#[derive(Debug, Clone, PartialEq)]
#[allow(dead_code)]
pub enum DialogType {
    None,
    Theme,
    Auth,
    Editor,
    Help,
}

#[derive(Debug)]
pub struct InputState {
    pub buffer: String,
    pub cursor_position: usize,
    pub history: VecDeque<String>,
    pub history_index: Option<usize>,
}

impl InputState {
    pub fn new() -> Self {
        Self {
            buffer: String::new(),
            cursor_position: 0,
            history: VecDeque::with_capacity(100),
            history_index: None,
        }
    }

    pub fn insert_char(&mut self, c: char) {
        self.buffer.insert(self.cursor_position, c);
        self.cursor_position += 1;
    }

    pub fn delete_char(&mut self) {
        if self.cursor_position > 0 {
            self.cursor_position -= 1;
            self.buffer.remove(self.cursor_position);
        }
    }

    pub fn clear(&mut self) {
        self.buffer.clear();
        self.cursor_position = 0;
        self.history_index = None;
    }

    pub fn submit(&mut self) -> String {
        let content = self.buffer.clone();
        if !content.trim().is_empty() {
            self.history.push_front(content.clone());
            if self.history.len() > 100 {
                self.history.pop_back();
            }
        }
        self.clear();
        content
    }

    pub fn move_cursor_left(&mut self) {
        if self.cursor_position > 0 {
            self.cursor_position -= 1;
        }
    }

    pub fn move_cursor_right(&mut self) {
        if self.cursor_position < self.buffer.len() {
            self.cursor_position += 1;
        }
    }

    pub fn move_cursor_home(&mut self) {
        self.cursor_position = 0;
    }

    pub fn move_cursor_end(&mut self) {
        self.cursor_position = self.buffer.len();
    }

    pub fn history_previous(&mut self) {
        if self.history.is_empty() {
            return;
        }

        let new_index = match self.history_index {
            None => 0,
            Some(i) if i < self.history.len() - 1 => i + 1,
            Some(i) => i,
        };

        if let Some(content) = self.history.get(new_index) {
            self.buffer = content.clone();
            self.cursor_position = self.buffer.len();
            self.history_index = Some(new_index);
        }
    }

    pub fn history_next(&mut self) {
        match self.history_index {
            Some(0) => {
                self.clear();
            }
            Some(i) => {
                self.history_index = Some(i - 1);
                if let Some(content) = self.history.get(i - 1) {
                    self.buffer = content.clone();
                    self.cursor_position = self.buffer.len();
                }
            }
            None => {}
        }
    }
}

#[derive(Debug)]
pub struct AppState {
    pub messages: Vec<Message>,
    pub input: InputState,
    #[allow(dead_code)]
    pub mode: AppMode,
    #[allow(dead_code)]
    pub dialog: DialogType,
    pub scroll_offset: usize,
    pub config: Config,
    pub is_streaming: bool,
    pub should_quit: bool,
    pub message_id_counter: usize,
    pub terminal_size: (u16, u16),
}

impl AppState {
    pub fn new(config: Config) -> Self {
        let mut state = Self {
            messages: Vec::new(),
            input: InputState::new(),
            mode: AppMode::Insert,
            dialog: DialogType::None,
            scroll_offset: 0,
            config,
            is_streaming: false,
            should_quit: false,
            message_id_counter: 0,
            terminal_size: (80, 24),
        };
        
        // Add welcome message
        state.add_message(
            MessageType::Info, 
            "Welcome to Assistant TUI! Type your message and press Enter to chat.".to_string()
        );
        
        state
    }

    pub fn add_message(&mut self, message_type: MessageType, content: String) {
        self.message_id_counter += 1;
        self.messages.push(Message {
            id: self.message_id_counter,
            message_type,
            content,
            timestamp: Utc::now(),
            is_streaming: false,
        });
        self.scroll_to_bottom();
    }

    pub fn start_streaming_message(&mut self, message_type: MessageType) -> usize {
        self.message_id_counter += 1;
        let id = self.message_id_counter;
        self.messages.push(Message {
            id,
            message_type,
            content: String::new(),
            timestamp: Utc::now(),
            is_streaming: true,
        });
        self.is_streaming = true;
        self.scroll_to_bottom();
        id
    }

    #[allow(dead_code)]
    pub fn update_streaming_message(&mut self, id: usize, content: &str) {
        if let Some(msg) = self.messages.iter_mut().find(|m| m.id == id) {
            msg.content = content.to_string();
        }
    }

    pub fn finish_streaming_message(&mut self, id: usize) {
        if let Some(msg) = self.messages.iter_mut().find(|m| m.id == id) {
            msg.is_streaming = false;
        }
        self.is_streaming = false;
    }

    pub fn scroll_up(&mut self, amount: usize) {
        self.scroll_offset = self.scroll_offset.saturating_add(amount);
    }

    pub fn scroll_down(&mut self, amount: usize) {
        self.scroll_offset = self.scroll_offset.saturating_sub(amount);
    }

    pub fn scroll_to_bottom(&mut self) {
        self.scroll_offset = 0;
    }

    pub fn update_terminal_size(&mut self, width: u16, height: u16) {
        self.terminal_size = (width, height);
    }
}