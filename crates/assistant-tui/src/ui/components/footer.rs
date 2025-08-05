use crate::state::AppState;
use ratatui::{
    buffer::Buffer,
    layout::{Alignment, Rect},
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Widget},
};

pub fn render_footer(area: Rect, buf: &mut Buffer, state: &AppState) {
    let mut spans = vec![];
    
    spans.push(Span::styled(
        format!("Model: {}", &state.config.model),
        Style::default().fg(Color::Green),
    ));
    
    spans.push(Span::raw(" | "));
    
    spans.push(Span::styled(
        "Ctrl+C: Exit | Ctrl+L: Clear | Ctrl+O: Errors | Tab: Complete",
        Style::default().fg(Color::DarkGray),
    ));
    
    let footer = Paragraph::new(Line::from(spans))
        .block(
            Block::default()
                .borders(Borders::TOP)
                .border_style(Style::default().fg(Color::DarkGray)),
        )
        .alignment(Alignment::Left);
    
    footer.render(area, buf);
}