use ratatui::{
    layout::Rect,
    prelude::{Alignment, Frame, Line, Span},
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, Clear, Paragraph},
};

use crate::app::App;
use crate::api::models::SongDetail;

pub fn render_song_info(frame: &mut Frame, app: &App) {
    let Some(overlay) = app.song_info_overlay.as_ref() else { return };

    let detail: Option<&SongDetail> = overlay.detail.as_ref();
    let fallback = &overlay.fallback_song;

    let title     = detail.map(|d| d.title.clone()).unwrap_or_else(|| fallback.title.clone());
    let artist    = detail.and_then(|d| d.artist.clone()).or_else(|| fallback.artist.clone());
    let album     = detail.and_then(|d| d.album.clone()).or_else(|| fallback.album.clone());
    let track     = detail.and_then(|d| d.track).or(fallback.track);
    let duration  = detail.map(|d| d.duration).unwrap_or(fallback.duration);
    let year      = detail.and_then(|d| d.year);
    let genre     = detail.and_then(|d| d.genre.clone());
    let suffix    = detail.and_then(|d| d.suffix.clone());
    let content_t = detail.and_then(|d| d.content_type.clone());
    let bitrate   = detail.and_then(|d| d.bit_rate);
    let size      = detail.and_then(|d| d.size);
    let path      = detail.and_then(|d| d.path.clone());
    let server_pc = detail.and_then(|d| d.play_count);
    let starred   = detail.and_then(|d| d.starred.as_ref()).is_some()
        || fallback.starred.is_some();

    let label = Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD);
    let head  = Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD);
    let dim   = Style::default().fg(Color::DarkGray);

    // Pre-compute every display string as an owned String.
    // This avoids E0716: temporary values dropped while borrowed.
    let track_str    = track.map(|t| t.to_string()).unwrap_or_else(|| "—".into());
    let year_str     = year.map(|y| y.to_string()).unwrap_or_else(|| "—".into());
    let duration_str = format!("{:02}:{:02}", duration / 60, duration % 60);
    let format_str   = match (&suffix, &content_t) {
        (Some(s), Some(c)) => format!("{} ({})", s.to_uppercase(), c),
        (Some(s), None)    => s.to_uppercase(),
        (None, Some(c))    => c.clone(),
        (None, None)       => "—".to_string(),
    };
    let bitrate_str  = bitrate.map(|b| format!("{} kbps", b)).unwrap_or_else(|| "—".into());
    let size_str     = size.map(format_size).unwrap_or_else(|| "—".into());
    let server_pc_str = server_pc.map(|c| c.to_string()).unwrap_or_else(|| "—".into());
    let local_pc_str  = overlay.local_play_count.to_string();

    let mut lines = vec![
        Line::from(Span::styled(" Song Info ", head)),
        Line::from(""),
        labeled("Title:    ", title.clone(), label),
        labeled("Artist:   ", artist.clone().unwrap_or_else(|| "—".into()), label),
        labeled("Album:    ", album.clone().unwrap_or_else(|| "—".into()), label),
        labeled("Track:    ", track_str, label),
        labeled("Year:     ", year_str, label),
        labeled("Genre:    ", genre.clone().unwrap_or_else(|| "—".into()), label),
        Line::from(""),
        labeled("Duration: ", duration_str, label),
        labeled("Format:   ", format_str, label),
        labeled("Bitrate:  ", bitrate_str, label),
        labeled("Size:     ", size_str, label),
        Line::from(""),
        labeled("Starred:  ", if starred { "❤️  Yes".to_string() } else { "No".to_string() }, label),
        labeled("Server plays: ", server_pc_str, label),
        labeled("Local plays:  ", local_pc_str, label),
    ];

    if let Some(p) = path.as_deref() {
        lines.push(Line::from(""));
        lines.push(labeled("Path:     ", p.to_string(), label));
    }

    if let Some(err) = &overlay.error {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            format!("  ⚠️  Extended info unavailable: {}", err),
            dim,
        )));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled("  Press ESC to close", dim)));

    let sz     = frame.size();
    let height = ((lines.len() as u16) + 2).min(sz.height.saturating_sub(2));
    let width  = 68u16.min(sz.width.saturating_sub(4));
    let x      = sz.width.saturating_sub(width) / 2;
    let y      = sz.height.saturating_sub(height) / 2;
    let area   = Rect { x, y, width, height };

    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(lines)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" Song Info ")
                    .border_style(Style::default().fg(Color::Magenta)),
            )
            .alignment(Alignment::Left),
        area,
    );
}

/// Build a single labeled line. `value` is owned so we never borrow a
/// short-lived temporary — this is what fixed the eight E0716 errors.
fn labeled(label: &str, value: String, style: Style) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("  {}", label), style),
        Span::raw(value),
    ])
}

fn format_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = 1024 * KB;
    const GB: u64 = 1024 * MB;
    if bytes >= GB      { format!("{:.2} GB", bytes as f64 / GB as f64) }
    else if bytes >= MB { format!("{:.1} MB", bytes as f64 / MB as f64) }
    else if bytes >= KB { format!("{:.1} KB", bytes as f64 / KB as f64) }
    else                { format!("{} B", bytes) }
}