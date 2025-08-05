use crate::state::{Message, MessageType};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, List, ListItem, Widget},
};

pub fn render_messages(area: Rect, buf: &mut Buffer, messages: &[Message], scroll_offset: usize) {
    let message_items: Vec<ListItem> = messages
        .iter()
        .skip(scroll_offset)
        .take(area.height as usize)
        .map(|msg| create_message_item(msg))
        .collect();
    
    let messages_list = List::new(message_items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Chat ")
                .border_style(Style::default().fg(Color::DarkGray)),
        );
    
    messages_list.render(area, buf);
}

fn create_message_item(message: &Message) -> ListItem {
    let (prefix, style) = match &message.message_type {
        MessageType::User => ("You", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
        MessageType::Assistant => ("Assistant", Style::default().fg(Color::Blue).add_modifier(Modifier::BOLD)),
        MessageType::Tool { name } => (name.as_str(), Style::default().fg(Color::Yellow)),
        MessageType::Error => ("Error", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
        MessageType::Info => ("Info", Style::default().fg(Color::Cyan)),
        MessageType::System => ("System", Style::default().fg(Color::DarkGray)),
    };
    
    let timestamp = message.timestamp.format("%H:%M:%S").to_string();
    
    let mut lines = vec![
        Line::from(vec![
            Span::styled(format!("[{}] ", timestamp), Style::default().fg(Color::DarkGray)),
            Span::styled(format!("{}: ", prefix), style),
        ])
    ];
    
    for (i, line) in message.content.lines().enumerate() {
        if i == 0 && !line.is_empty() {
            if let Some(last_line) = lines.last_mut() {
                last_line.spans.push(Span::raw(line));
            }
        } else {
            lines.push(Line::from(format!("        {}", line)));
        }
    }
    
    // Don't show visual cursor for streaming messages
    
    ListItem::new(Text::from(lines))
}