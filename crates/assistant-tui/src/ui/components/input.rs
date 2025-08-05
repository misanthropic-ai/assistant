use crate::state::InputState;
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Widget},
};

pub fn render_input(area: Rect, buf: &mut Buffer, input: &InputState, is_active: bool) {
    let prompt = if is_active { "❯ " } else { "  " };
    
    let cursor_style = if is_active {
        Style::default().bg(Color::White).fg(Color::Black)
    } else {
        Style::default()
    };
    
    let mut spans = vec![
        Span::styled(prompt, Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
    ];
    
    let before_cursor = &input.buffer[..input.cursor_position];
    let at_cursor = input.buffer.chars().nth(input.cursor_position).unwrap_or(' ');
    let after_cursor = if input.cursor_position < input.buffer.len() {
        &input.buffer[input.cursor_position + at_cursor.len_utf8()..]
    } else {
        ""
    };
    
    if !before_cursor.is_empty() {
        spans.push(Span::raw(before_cursor));
    }
    
    spans.push(Span::styled(at_cursor.to_string(), cursor_style));
    
    if !after_cursor.is_empty() {
        spans.push(Span::raw(after_cursor));
    }
    
    let input_widget = Paragraph::new(Line::from(spans))
        .block(
            Block::default()
                .borders(Borders::TOP | Borders::LEFT | Borders::RIGHT)
                .border_style(
                    Style::default().fg(if is_active { Color::Cyan } else { Color::DarkGray })
                )
                .title(if input.buffer.is_empty() {
                    " Type your message or @file "
                } else {
                    ""
                }),
        );
    
    input_widget.render(area, buf);
}