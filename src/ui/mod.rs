pub mod panels;
pub mod jukebox_panels;
pub mod help;
pub mod search_input;

use ratatui::{
    layout::{Constraint, Layout},
    prelude::{Alignment, Frame, Line, Span},
    style::{Color, Modifier, Style},
    widgets::{Paragraph},
};
use std::sync::atomic::Ordering;

use crate::app::{App, ViewMode};
use panels::*;
use jukebox_panels::*;
use help::render_help;
use search_input::render_search_input;

pub fn ui(frame: &mut Frame, app: &App) {
    if app.is_help_mode {
        render_help(frame);
    } else if app.is_search_mode {
        render_search_input(frame, app);
    } else if app.mode == ViewMode::Visualizer {
        app.visualizer.render(frame, frame.size());
    } else {
        render_main(frame, app);
    }
}

fn render_main(frame: &mut Frame, app: &App) {
    let main_layout = Layout::vertical([
        Constraint::Min(3),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
    ]).split(frame.size());

    let panels = Layout::horizontal([
        Constraint::Ratio(2, 6),
        Constraint::Ratio(2, 6),
        Constraint::Ratio(2, 6),
    ]).split(main_layout[0]);

    if app.is_jukebox_mode {
        render_jukebox_left_panel(frame, app, panels[0]);
        render_jukebox_center_panel(frame, app, panels[1]);
        render_songs_panel(frame, app, panels[2]);
    } else {
        match app.mode {
            ViewMode::Playlists | ViewMode::PlaylistSongs => {
                render_playlists_panel(frame, app, panels[0]);
                render_playlist_context_panel(frame, app, panels[1]);
            }
            _ => {
                render_artists_panel(frame, app, panels[0]);
                render_albums_panel(frame, app, panels[1]);
            }
        }
        render_songs_panel(frame, app, panels[2]);
    }

    let divider       = "─".repeat(frame.size().width as usize);
    let divider_style = Style::default().fg(Color::DarkGray);
    frame.render_widget(Paragraph::new(divider.clone()).style(divider_style), main_layout[1]);
    frame.render_widget(Paragraph::new(divider).style(divider_style),         main_layout[3]);

    // Status bar
    let mute_str = if app.is_muted { "ON" } else { "OFF" };
    let mut status_spans = vec![
        Span::styled(format!("VOL:{}% ", app.volume), Style::new().fg(Color::Cyan)),
        Span::raw("| "),
        Span::styled("MUTE:", Style::new().fg(Color::Magenta)),
        Span::styled(mute_str, Style::new().fg(if app.is_muted { Color::Red } else { Color::Green })),
        Span::raw(" | "),
        Span::styled("/", Style::new().fg(Color::Yellow)),
        Span::styled(":Search", Style::new().fg(Color::DarkGray)),
        Span::raw(" | "),
        Span::styled("Q", Style::new().fg(Color::Red).add_modifier(Modifier::BOLD)),
        Span::styled(":Quit", Style::new().fg(Color::DarkGray)),
        Span::raw(" | "),
        Span::styled("H", Style::new().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
        Span::styled(":Help", Style::new().fg(Color::DarkGray)),
        Span::raw(" | "),
        Span::styled("L", Style::new().fg(Color::Red).add_modifier(Modifier::BOLD)),
        Span::styled(":❤️ Like", Style::new().fg(Color::DarkGray)),
        Span::raw(" | "),
        Span::styled("n/p", Style::new().fg(Color::Cyan)),
        Span::styled(":Tracks", Style::new().fg(Color::DarkGray)),
        Span::raw(" | "),
        Span::styled("Tab", Style::new().fg(Color::Magenta).add_modifier(Modifier::BOLD)),
        Span::styled(":Mode", Style::new().fg(Color::DarkGray)),
        Span::raw(" | "),
        Span::styled("J", Style::new().fg(Color::Green).add_modifier(Modifier::BOLD)),
        Span::styled(":Juke", Style::new().fg(Color::DarkGray)),
        Span::raw(" | "),
        Span::styled("S", Style::new().fg(Color::Magenta).add_modifier(Modifier::BOLD)),
        Span::styled(":Shuffle", Style::new().fg(Color::DarkGray)),
    ];
    if app.is_jukebox_mode {
        status_spans.push(Span::raw(" | "));
        status_spans.push(Span::styled("🎉 JUKEBOX", Style::new().fg(Color::Green).add_modifier(Modifier::BOLD)));
    }
    if app.is_shuffle {
        status_spans.push(Span::raw(" | "));
        status_spans.push(Span::styled("🔀 SHUFFLE", Style::new().fg(Color::Magenta).add_modifier(Modifier::BOLD)));
    }
    frame.render_widget(Paragraph::new(Line::from(status_spans)), main_layout[2]);

    // Now playing info
    let song_info = app.now_playing
        .and_then(|i| app.songs.get(i))
        .map(|song| {
            let prefix = if app.is_jukebox_mode { "🎉" } else if app.is_shuffle { "🔀" } else { "▶" };
            format!("{} {} - {}", prefix, song.artist.as_deref().unwrap_or("Unknown"), song.title)
        })
        .unwrap_or_else(|| "⏹ Stopped".into());

    frame.render_widget(
        Paragraph::new(song_info).style(Style::default().fg(
            if app.is_jukebox_mode { Color::Green } else if app.is_shuffle { Color::Magenta } else { Color::Yellow }
        )),
        main_layout[4],
    );

    // Progress bar
    let (current, total) = app.now_playing
        .and_then(|i| app.songs.get(i))
        .map(|song| (
            (app.player_status.current_time.load(Ordering::Relaxed) as u64) / 1000,
            song.duration,
        ))
        .unwrap_or((0, 1));

    let bar_width = (frame.size().width as usize).saturating_sub(20).max(10);
    let filled    = ((current as f32 / total.max(1) as f32 * bar_width as f32).round() as usize).min(bar_width);
    let progress_bar = format!(
        "{:02}:{:02} ┃{}{}┃ {:02}:{:02}",
        current / 60, current % 60,
        "━".repeat(filled),
        "─".repeat(bar_width.saturating_sub(filled)),
        total / 60, total % 60,
    );
    frame.render_widget(
        Paragraph::new(progress_bar)
            .style(Style::default().fg(
                if app.is_jukebox_mode { Color::Green } else if app.is_shuffle { Color::Magenta } else { Color::Blue }
            ))
            .alignment(Alignment::Center),
        main_layout[5],
    );
}
