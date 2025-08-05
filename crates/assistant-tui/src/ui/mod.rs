pub mod components;

use crate::state::AppState;
use ratatui::{
    layout::{Constraint, Direction, Layout},
    Frame,
};

pub fn render(frame: &mut Frame, state: &AppState) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(6),  // Header
            Constraint::Min(10),    // Messages
            Constraint::Length(3),  // Input
            Constraint::Length(2),  // Footer
        ])
        .split(frame.area());

    components::render_header(chunks[0], frame.buffer_mut(), env!("CARGO_PKG_VERSION"), false);
    components::render_messages(chunks[1], frame.buffer_mut(), &state.messages, state.scroll_offset);
    components::render_input(chunks[2], frame.buffer_mut(), &state.input, !state.is_streaming);
    components::render_footer(chunks[3], frame.buffer_mut(), state);
}