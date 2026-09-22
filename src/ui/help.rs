use ratatui::{
    layout::{Constraint, Layout, Rect},
    prelude::{Alignment, Frame, Line, Span},
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, Paragraph, Wrap},
};

pub fn render_help(frame: &mut Frame) {
    let area = frame.size();

    // Fullscreen help: split into two columns. On a very narrow terminal
    // we fall back to a single column so nothing is cut off.
    let columns = if area.width >= 72 {
        Layout::horizontal([
            Constraint::Ratio(1, 2),
            Constraint::Ratio(1, 2),
        ]).split(area)
    } else {
        // Single-column fallback — reuse the left column and leave the
        // right one empty so the caller still gets the same shape.
        Layout::horizontal([
            Constraint::Ratio(1, 1),
            Constraint::Ratio(0, 1),
        ]).split(area)
    };

    render_left_column(frame, columns[0]);
    if area.width >= 72 {
        render_right_column(frame, columns[1]);
    }
}

fn render_left_column(frame: &mut Frame, area: Rect) {
    let header_style = Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD);
    let key_style    = Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD);
    let desc_style   = Style::default().fg(Color::Gray);
    let dim          = Style::default().fg(Color::DarkGray);

    let lines = vec![
        Line::from(Span::styled(" TerminalDrome — Keyboard Shortcuts ", header_style)),
        Line::from(""),

        Line::from(Span::styled("▶ Navigation", header_style)),
        key_line("↑ / ↓", "Move selection up / down", key_style, desc_style),
        key_line("← / →", "Switch between views", key_style, desc_style),
        key_line("Enter", "Confirm / start playback", key_style, desc_style),
        key_line("Tab", "Toggle Artists ↔ Playlists", key_style, desc_style),
        key_line("A–Z", "Quick jump to first entry", key_style, desc_style),
        Line::from(""),

        Line::from(Span::styled("▶ Playback", header_style)),
        key_line("Space", "Stop playback", key_style, desc_style),
        key_line("n / p", "Next / previous track", key_style, desc_style),
        key_line("+ / -", "Volume up / down", key_style, desc_style),
        key_line("m", "Toggle mute", key_style, desc_style),
        key_line("Shift+S", "Shuffle current list & restart", key_style, desc_style),
        key_line("Shift+L", "❤️  Like current song", key_style, desc_style),
        Line::from(""),

        Line::from(Span::styled("▶ Playlists", header_style)),
        key_line("a", "Add current song to a playlist", key_style, desc_style),
        key_line("Shift+N", "Create new playlist (from picker)", key_style, desc_style),
        key_line("d", "Remove song from open playlist", key_style, desc_style),
        Line::from(""),

        Line::from(Span::styled("  Press any key to close", dim)),
    ];

    frame.render_widget(
        Paragraph::new(lines)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" Help ")
                    .border_style(Style::default().fg(Color::LightBlue)),
            )
            .alignment(Alignment::Left)
            .wrap(Wrap { trim: false }),
        area,
    );
}

fn render_right_column(frame: &mut Frame, area: Rect) {
    let header_style = Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD);
    let key_style    = Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD);
    let desc_style   = Style::default().fg(Color::Gray);
    let dim          = Style::default().fg(Color::DarkGray);

    let lines = vec![
        Line::from(Span::styled(" Modes & Other ", header_style)),
        Line::from(""),

        Line::from(Span::styled("▶ Modes", header_style)),
        key_line("Shift+B", "Toggle source (Navidrome / Bandcamp)", key_style, desc_style),
        key_line("Shift+J", "Start Jukebox / Party Mode", key_style, desc_style),
        key_line("Shift+E", "Toggle audio visualizer", key_style, desc_style),
        key_line("ESC", "Exit Jukebox / close Visualizer", key_style, desc_style),
        Line::from(""),

        Line::from(Span::styled("▶ Song Info", header_style)),
        key_line("Shift+I", "Show song info (bitrate, format, plays)", key_style, desc_style),
        key_line("ESC", "Close song info overlay", key_style, desc_style),
        Line::from(""),

        Line::from(Span::styled("▶ Search & Quit", header_style)),
        key_line("/", "Search across your library", key_style, desc_style),
        key_line("Shift+H", "This help screen", key_style, desc_style),
        key_line("Shift+Q", "Quit TerminalDrome", key_style, desc_style),
        Line::from(""),

        Line::from(Span::styled("▶ Playlist Picker", header_style)),
        key_line("↑ / ↓", "Select playlist", key_style, desc_style),
        key_line("Enter", "Confirm", key_style, desc_style),
        key_line("Shift+N", "New playlist", key_style, desc_style),
        key_line("ESC", "Cancel", key_style, desc_style),
        Line::from(""),

        Line::from(Span::styled("  Universal: H help  •  Q quit", dim)),
    ];

    frame.render_widget(
        Paragraph::new(lines)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" Help ")
                    .border_style(Style::default().fg(Color::LightBlue)),
            )
            .alignment(Alignment::Left)
            .wrap(Wrap { trim: false }),
        area,
    );
}

/// Build one "key — description" line with consistent column widths.
fn key_line(key: &str, desc: &str, key_style: Style, desc_style: Style) -> Line<'static> {
    Line::from(vec![
        Span::raw("  "),
        Span::styled(format!("{:<9}", key), key_style),
        Span::styled(desc.to_string(), desc_style),
    ])
}