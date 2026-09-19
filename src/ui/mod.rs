pub mod panels;
pub mod jukebox_panels;
pub mod help;
pub mod search_input;
pub mod song_info;
use song_info::render_song_info;

use ratatui::{
    layout::{Constraint, Layout},
    prelude::{Alignment, Frame, Line, Span},
    style::{Color, Modifier, Style},
    widgets::Paragraph,
};
use std::sync::atomic::Ordering;

use crate::app::{App, ViewMode};
use crate::api::endpoints::MusicSource;
use panels::*;
use jukebox_panels::*;
use help::render_help;
use search_input::render_search_input;

pub fn ui(frame: &mut Frame, app: &App) {
    if app.is_help_mode {
        render_help(frame);
    } else if app.is_search_mode {
        render_search_input(frame, app);
    } else if app.song_info_overlay.is_some() {
        render_main(frame, app);
        render_song_info(frame, app);
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

    // Status bar — compact, context-sensitive, three zones
    let status = build_status_bar(app, frame.size().width);
    frame.render_widget(Paragraph::new(status), main_layout[2]);

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

// ── Status bar ────────────────────────────────────────────────────────────────

/// Build the status line as three zones:
///
///   [ left: source / badges / volume ] │ [ center: context keys ] ... [ right: H, Q ]
///
/// The center zone adapts to the current view mode. Universal keys (`H`, `Q`)
/// are always available, so they live right-aligned and never compete for
/// space with contextual hints.
fn build_status_bar(app: &App, total_width: u16) -> Line<'static> {
    let key_style   = Style::new().fg(Color::Yellow).add_modifier(Modifier::BOLD);
    let label_style = Style::new().fg(Color::DarkGray);

    // ── Left zone ─────────────────────────────────────────────────────────
    let mut left: Vec<Span<'static>> = Vec::new();

    let (source_icon, source_color) = match app.active_source {
        MusicSource::Navidrome => ("🤖 ND", Color::LightGreen),
        MusicSource::Bandcamp  => ("🎸 BC", Color::LightMagenta),
    };
    left.push(Span::styled(source_icon, Style::new().fg(source_color).add_modifier(Modifier::BOLD)));

    if app.is_jukebox_mode {
        left.push(Span::raw(" │ "));
        left.push(Span::styled("🎉 JUKEBOX", Style::new().fg(Color::Green).add_modifier(Modifier::BOLD)));
    } else if app.is_shuffle {
        left.push(Span::raw(" │ "));
        left.push(Span::styled("🔀 SHUFFLE", Style::new().fg(Color::Magenta).add_modifier(Modifier::BOLD)));
    }

    left.push(Span::raw(" │ "));
    if app.is_muted {
        left.push(Span::styled("🔇 muted", Style::new().fg(Color::Red)));
    } else {
        left.push(Span::styled(format!("🔊{}%", app.volume), Style::new().fg(Color::Cyan)));
    }

    // ── Right zone ────────────────────────────────────────────────────────
    let mut right: Vec<Span<'static>> = Vec::new();
    right.push(Span::styled("H", key_style));
    right.push(Span::styled(":help", label_style));
    right.push(Span::raw("  "));
    right.push(Span::styled("Q", Style::new().fg(Color::Red).add_modifier(Modifier::BOLD)));
    right.push(Span::styled(":quit", label_style));

    // ── Center zone — context-sensitive ───────────────────────────────────
    let hints: Vec<(&str, &str, Color)> = match app.mode {
        ViewMode::Artists => vec![
            ("↑↓",  "sel",       Color::Cyan),
            ("→",   "open",      Color::Yellow),
            ("Tab", "playlists", Color::Magenta),
            ("/",   "find",      Color::Yellow),
        ],
        ViewMode::Albums => vec![
            ("↑↓",  "sel",       Color::Cyan),
            ("→",   "play",      Color::Yellow),
            ("←",   "back",      Color::DarkGray),
            ("Tab", "playlists", Color::Magenta),
            ("/",   "find",      Color::Yellow),
        ],
        ViewMode::Songs | ViewMode::PlaylistSongs => vec![
            ("↑↓",  "sel",  Color::Cyan),
            ("⏎",   "play", Color::Yellow),
            ("␣",   "stop", Color::Yellow),
            ("n/p", "trk",  Color::Cyan),
            ("S",   "🔀",   Color::Magenta),
            ("I",   "info", Color::Yellow),
            ("L",   "❤️",   Color::Red),
        ],
        ViewMode::Playlists => vec![
            ("↑↓",  "sel",     Color::Cyan),
            ("→",   "open",    Color::Yellow),
            ("Tab", "artists", Color::Magenta),
            ("/",   "find",    Color::Yellow),
        ],
        ViewMode::Jukebox => vec![
            ("␣",   "stop", Color::Yellow),
            ("n/p", "trk",  Color::Cyan),
            ("ESC", "exit", Color::Yellow),
            ("I",   "info", Color::Yellow),
            ("L",   "❤️",   Color::Red),
        ],
        ViewMode::Visualizer => vec![
            ("ESC", "close", Color::Yellow),
        ],
    };

    let bandcamp_available = app.config.bandcamp
        .as_ref()
        .map_or(false, |bc| bc.enabled);

    let mut center: Vec<Span<'static>> = Vec::new();
    for (i, (key, desc, color)) in hints.iter().enumerate() {
        if i > 0 {
            center.push(Span::styled(" │ ", label_style));
        }
        center.push(Span::styled(key.to_string(), Style::new().fg(*color).add_modifier(Modifier::BOLD)));
        if !desc.is_empty() {
            center.push(Span::raw(":"));
            center.push(Span::styled(desc.to_string(), label_style));
        }
    }
    if bandcamp_available && !matches!(app.mode, ViewMode::Visualizer) {
        center.push(Span::styled(" │ ", label_style));
        center.push(Span::styled("B", Style::new().fg(Color::Magenta).add_modifier(Modifier::BOLD)));
        center.push(Span::raw(":"));
        center.push(Span::styled("src", label_style));
    }

    // ── Compose the three zones, padding between center and right ─────────
    let left_w:   usize = left.iter().map(|s| s.width()).sum();
    let center_w: usize = center.iter().map(|s| s.width()).sum();
    let right_w:  usize = right.iter().map(|s| s.width()).sum();

    // left + " │ " + center + pad + right = total_width
    let used = left_w + 3 + center_w + right_w;
    let pad  = (total_width as usize).saturating_sub(used);

    let mut spans = left;
    spans.push(Span::raw(" │ "));
    spans.extend(center);
    if pad > 0 {
        spans.push(Span::raw(" ".repeat(pad)));
    } else {
        // Content overflowed — keep a minimal two-space gap so the right
        // side doesn't jam up against the center.
        spans.push(Span::raw("  "));
    }
    spans.extend(right);

    Line::from(spans)
}