pub mod components;

use crate::state::{AppState, ViewMode};
use ratatui::{
    layout::{Constraint, Direction, Layout},
    Frame,
};

pub fn render(frame: &mut Frame, state: &AppState) {
    match &state.view_mode {
        ViewMode::Chat => render_chat_view(frame, state),
        ViewMode::ConversationList => render_conversation_list_view(frame, state),
        ViewMode::RenameDialog { session_id } => render_rename_dialog(frame, state, session_id),
        ViewMode::DeleteConfirmation { session_id } => render_delete_confirmation(frame, state, session_id),
    }
}

fn render_chat_view(frame: &mut Frame, state: &AppState) {
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
    
    // Always set cursor position in the input field
    // Calculate cursor position: x = border(1) + prompt(2) + cursor position in buffer
    let cursor_x = chunks[2].x + 1 + 2 + state.input.cursor_position as u16;
    // y = input area start + border(1)
    let cursor_y = chunks[2].y + 1;
    frame.set_cursor_position((cursor_x, cursor_y));
}

fn render_conversation_list_view(frame: &mut Frame, state: &AppState) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(6),  // Header
            Constraint::Min(10),    // Conversation list
            Constraint::Length(4),  // Actions
        ])
        .split(frame.area());

    components::render_header(chunks[0], frame.buffer_mut(), env!("CARGO_PKG_VERSION"), false);
    components::render_conversation_list(chunks[1], frame.buffer_mut(), &state.conversation_list);
    components::render_conversation_actions(chunks[2], frame.buffer_mut());
}

fn render_rename_dialog(frame: &mut Frame, state: &AppState, _session_id: &str) {
    // TODO: Implement rename dialog
    render_chat_view(frame, state);
}

fn render_delete_confirmation(frame: &mut Frame, state: &AppState, session_id: &str) {
    use ratatui::{
        style::{Color, Modifier, Style},
        text::{Line, Span},
        widgets::{Block, Borders, Clear, Paragraph, Wrap},
    };
    
    // Render the chat view in the background
    render_chat_view(frame, state);
    
    // Calculate centered position for dialog
    let area = frame.area();
    let dialog_width = 60.min(area.width - 4);
    let dialog_height = 8.min(area.height - 4);
    let x = (area.width - dialog_width) / 2;
    let y = (area.height - dialog_height) / 2;
    
    let dialog_area = ratatui::layout::Rect::new(x, y, dialog_width, dialog_height);
    
    // Clear the area for the dialog
    frame.render_widget(Clear, dialog_area);
    
    // Find the conversation name
    let conversation_name = state.conversation_list.conversations
        .iter()
        .find(|c| c.id == session_id)
        .and_then(|c| c.name.as_ref())
        .map(|s| s.as_str())
        .unwrap_or("this conversation");
    
    let text = vec![
        Line::from(""),
        Line::from(vec![
            Span::raw("Are you sure you want to delete "),
            Span::styled(conversation_name, Style::default().add_modifier(Modifier::BOLD)),
            Span::raw("?"),
        ]),
        Line::from(""),
        Line::from("This action cannot be undone."),
        Line::from(""),
        Line::from(vec![
            Span::styled("Y", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
            Span::raw("es / "),
            Span::styled("N", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
            Span::raw("o"),
        ]),
    ];
    
    let paragraph = Paragraph::new(text)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Delete Confirmation ")
                .title_style(Style::default().fg(Color::Red).add_modifier(Modifier::BOLD))
                .border_style(Style::default().fg(Color::Red)),
        )
        .wrap(Wrap { trim: true })
        .alignment(ratatui::layout::Alignment::Center);
    
    frame.render_widget(paragraph, dialog_area);
}