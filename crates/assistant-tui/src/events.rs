use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

#[derive(Debug, Clone)]
pub enum AppEvent {
    Key(KeyEvent),
    Resize(u16, u16),
    Tick,
}

pub struct EventHandler {
    rx: mpsc::Receiver<AppEvent>,
    _handler_thread: thread::JoinHandle<()>,
}

impl EventHandler {
    pub fn new(tick_rate: Duration) -> Self {
        let (tx, rx) = mpsc::channel();
        let tx_clone = tx.clone();

        let handler_thread = thread::spawn(move || {
            let mut last_tick = std::time::Instant::now();
            
            loop {
                let timeout = tick_rate
                    .checked_sub(last_tick.elapsed())
                    .unwrap_or_else(|| Duration::from_secs(0));

                if event::poll(timeout).unwrap_or(false) {
                    match event::read() {
                        Ok(Event::Key(key)) => {
                            if tx.send(AppEvent::Key(key)).is_err() {
                                break;
                            }
                        }
                        Ok(Event::Resize(width, height)) => {
                            if tx.send(AppEvent::Resize(width, height)).is_err() {
                                break;
                            }
                        }
                        _ => {}
                    }
                }

                if last_tick.elapsed() >= tick_rate {
                    if tx_clone.send(AppEvent::Tick).is_err() {
                        break;
                    }
                    last_tick = std::time::Instant::now();
                }
            }
        });

        Self {
            rx,
            _handler_thread: handler_thread,
        }
    }

    pub fn recv(&self) -> Result<AppEvent, mpsc::RecvError> {
        self.rx.recv()
    }
}

pub fn handle_key_event(key: KeyEvent) -> Option<Action> {
    match (key.code, key.modifiers) {
        (KeyCode::Char('c'), KeyModifiers::CONTROL) => Some(Action::Quit),
        (KeyCode::Char('l'), KeyModifiers::CONTROL) => Some(Action::ClearScreen),
        (KeyCode::Char('o'), KeyModifiers::CONTROL) => Some(Action::ToggleErrorDetails),
        (KeyCode::Char('t'), KeyModifiers::CONTROL) => Some(Action::ToggleToolDescriptions),
        (KeyCode::Tab, _) => Some(Action::Autocomplete),
        (KeyCode::Up, _) => Some(Action::HistoryPrevious),
        (KeyCode::Down, _) => Some(Action::HistoryNext),
        (KeyCode::Left, _) => Some(Action::CursorLeft),
        (KeyCode::Right, _) => Some(Action::CursorRight),
        (KeyCode::Home, _) => Some(Action::CursorHome),
        (KeyCode::End, _) => Some(Action::CursorEnd),
        (KeyCode::Enter, _) => Some(Action::Submit),
        (KeyCode::Backspace, _) => Some(Action::DeleteChar),
        (KeyCode::Char(c), _) => Some(Action::InsertChar(c)),
        (KeyCode::PageUp, _) => Some(Action::ScrollUp),
        (KeyCode::PageDown, _) => Some(Action::ScrollDown),
        (KeyCode::Esc, _) => Some(Action::Escape),
        _ => None,
    }
}

#[derive(Debug, Clone)]
pub enum Action {
    Quit,
    ClearScreen,
    ToggleErrorDetails,
    ToggleToolDescriptions,
    Autocomplete,
    HistoryPrevious,
    HistoryNext,
    CursorLeft,
    CursorRight,
    CursorHome,
    CursorEnd,
    Submit,
    DeleteChar,
    InsertChar(char),
    ScrollUp,
    ScrollDown,
    Escape,
}