use crate::state::ConversationListState;
use assistant_core::persistence::schema::SessionSummary;
use chrono::{DateTime, Local, Utc};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Widget},
};

pub fn render_conversation_list(area: Rect, buf: &mut Buffer, state: &ConversationListState) {
    let conversations = &state.conversations;
    let selected = state.selected_index;
    
    let items: Vec<ListItem> = conversations
        .iter()
        .enumerate()
        .skip(state.list_scroll_offset)
        .take(area.height.saturating_sub(2) as usize)
        .map(|(idx, session)| {
            let is_selected = idx == selected;
            create_conversation_item(session, is_selected)
        })
        .collect();
    
    let title = if state.is_searching {
        format!(" Search: {} ", state.search_query)
    } else {
        " Conversations ".to_string()
    };
    
    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(title)
                .border_style(
                    Style::default().fg(if state.is_searching { 
                        Color::Yellow 
                    } else { 
                        Color::Cyan 
                    })
                ),
        )
        .highlight_style(
            Style::default()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        );
    
    list.render(area, buf);
}

fn create_conversation_item(session: &SessionSummary, is_selected: bool) -> ListItem {
    let name = session.name.as_deref().unwrap_or("Untitled Conversation");
    let timestamp = format_timestamp(&session.last_accessed);
    let message_count = format!("{} messages", session.message_count);
    
    let mut line = vec![
        Span::raw(if is_selected { " > " } else { "   " }),
        Span::styled(
            format!("[{}] ", timestamp),
            Style::default().fg(Color::DarkGray),
        ),
        Span::styled(
            name,
            Style::default()
                .fg(if is_selected { Color::White } else { Color::Gray })
                .add_modifier(if is_selected { Modifier::BOLD } else { Modifier::empty() }),
        ),
        Span::raw("  "),
        Span::styled(
            format!("({})", message_count),
            Style::default().fg(Color::DarkGray),
        ),
    ];
    
    if let Some(summary) = &session.summary {
        let preview = if summary.len() > 50 {
            format!("{}...", &summary[..50])
        } else {
            summary.clone()
        };
        line.push(Span::raw("\n     "));
        line.push(Span::styled(
            preview,
            Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC),
        ));
    }
    
    ListItem::new(Line::from(line))
}

fn format_timestamp(timestamp: &DateTime<Utc>) -> String {
    let local_time = timestamp.with_timezone(&Local);
    let now = Local::now();
    let duration = now.signed_duration_since(local_time);
    
    if duration.num_minutes() < 1 {
        "just now".to_string()
    } else if duration.num_minutes() < 60 {
        let mins = duration.num_minutes();
        format!("{} min ago • {}", mins, local_time.format("%H:%M"))
    } else if duration.num_hours() < 24 {
        let hours = duration.num_hours();
        format!("{} hours ago • {}", hours, local_time.format("%H:%M"))
    } else if duration.num_days() == 1 {
        format!("Yesterday • {}", local_time.format("%H:%M"))
    } else if duration.num_days() < 7 {
        let days = duration.num_days();
        format!("{} days ago • {}", days, local_time.format("%H:%M"))
    } else {
        local_time.format("%Y-%m-%d %H:%M").to_string()
    }
}