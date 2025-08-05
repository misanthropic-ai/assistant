use ratatui::{
    buffer::Buffer,
    layout::{Alignment, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Widget},
};

const SHORT_LOGO: &str = r#"
    ___         _     __            __  
   / _ | ___ __(_)__ / /____ ____  / /_ 
  / __ |(_-<(_-< (_-</ __/ _ `/ _ \/ __/ 
 /_/ |_/___/___/___/\__/\_,_/_//_/\__/  
"#;

const LONG_LOGO: &str = r#"
    ___            _      __              __     _____ __    ____
   / _ | ___ _____(_)____/ /_____ _____  / /_   / ___// /   /  _/
  / __ |(_-<(_-< / (_-< / __/ _ `/ _ \/ __/  / /__ / /__ _/ /  
 /_/ |_/___/___/_/___/_/\__/\_,_/_//_/\__/   \___//____//___/  
"#;

pub fn render_header(area: Rect, buf: &mut Buffer, version: &str, nightly: bool) {
    let logo = if area.width > 80 { LONG_LOGO } else { SHORT_LOGO };
    
    let logo_lines: Vec<Line> = logo
        .lines()
        .skip(1)
        .map(|line| {
            Line::from(vec![
                Span::styled(line, Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
            ])
        })
        .collect();

    let mut lines = logo_lines;
    
    if nightly {
        lines.push(Line::from(vec![
            Span::styled(
                format!("v{}", version),
                Style::default().fg(Color::Yellow).add_modifier(Modifier::ITALIC)
            )
        ]));
    }

    let header = Paragraph::new(lines)
        .block(Block::default().borders(Borders::NONE))
        .alignment(Alignment::Center);

    header.render(area, buf);
}