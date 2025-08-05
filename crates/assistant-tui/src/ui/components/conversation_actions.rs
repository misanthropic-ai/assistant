use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Widget},
};

pub fn render_conversation_actions(area: Rect, buf: &mut Buffer) {
    let actions = vec![
        Line::from(vec![
            Span::styled("Enter", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            Span::raw(": Continue  "),
            Span::styled("d", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
            Span::raw(": Delete  "),
            Span::styled("r", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
            Span::raw(": Rename"),
        ]),
        Line::from(vec![
            Span::styled("n", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
            Span::raw(": New  "),
            Span::styled("/", Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD)),
            Span::raw(": Search  "),
            Span::styled("Esc", Style::default().fg(Color::DarkGray).add_modifier(Modifier::BOLD)),
            Span::raw(": Back"),
        ]),
    ];
    
    let paragraph = Paragraph::new(actions)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Actions ")
                .border_style(Style::default().fg(Color::DarkGray)),
        );
    
    paragraph.render(area, buf);
}