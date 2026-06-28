use ratatui::{
    layout::Rect,
    prelude::{Alignment, Frame, Line, Span},
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, Paragraph},
};
use std::sync::atomic::Ordering;
use crate::app::App;

pub fn render_jukebox_left_panel(frame: &mut Frame, _app: &App, area: Rect) {
    let lines = vec![
        Line::from(""),
        Line::from(Span::styled("  🎉 Party / Jukebox Mode", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD))),
        Line::from(""),
        Line::from(Span::styled("  Your entire library", Style::default().fg(Color::White))),
        Line::from(Span::styled("  will be played in random order.", Style::default().fg(Color::White))),
        Line::from(""),
        Line::from(Span::styled("  Songs are automatically", Style::default().fg(Color::DarkGray))),
        Line::from(Span::styled("  loaded in the background.", Style::default().fg(Color::DarkGray))),
        Line::from(""),
        Line::from(Span::styled("  ESC  – Jukebox End", Style::default().fg(Color::Yellow))),
        Line::from(Span::styled("  n/p  – Next/Previous", Style::default().fg(Color::Yellow))),
        Line::from(Span::styled("  Spc  – Stop", Style::default().fg(Color::Yellow))),
    ];
    frame.render_widget(
        Paragraph::new(lines)
            .block(Block::default().title(" 🎉 Jukebox ").borders(Borders::ALL).border_style(Style::default().fg(Color::Green)))
            .alignment(Alignment::Left),
        area,
    );
}

pub fn render_jukebox_center_panel(frame: &mut Frame, app: &App, area: Rect) {
    let queued    = app.songs.len();
    let current   = app.player_status.current_index.load(Ordering::Acquire);
    let remaining = if current != usize::MAX { queued.saturating_sub(current) } else { queued };
    let now_artist = app.now_playing.and_then(|i| app.songs.get(i)).and_then(|s| s.artist.as_deref()).unwrap_or("–");
    let now_album  = app.now_playing.and_then(|i| app.songs.get(i)).and_then(|s| s.album.as_deref()).unwrap_or("–");

    let lines = vec![
        Line::from(""),
        Line::from(vec![Span::styled("  Artist:  ", Style::default().fg(Color::Cyan)), Span::raw(now_artist)]),
        Line::from(vec![Span::styled("  Album:   ", Style::default().fg(Color::Cyan)), Span::raw(now_album)]),
        Line::from(""),
        Line::from(vec![Span::styled("  In Queue:    ", Style::default().fg(Color::DarkGray)), Span::styled(format!("{}", queued), Style::default().fg(Color::White))]),
        Line::from(vec![Span::styled("  Remaining:   ", Style::default().fg(Color::DarkGray)), Span::styled(format!("{}", remaining), Style::default().fg(Color::White))]),
        Line::from(""),
        Line::from(Span::styled("  Reload: automatic", Style::default().fg(Color::DarkGray))),
    ];
    frame.render_widget(
        Paragraph::new(lines)
            .block(Block::default().title(" Now Playing ").borders(Borders::ALL).border_style(Style::default().fg(Color::Green)))
            .alignment(Alignment::Left),
        area,
    );
}
