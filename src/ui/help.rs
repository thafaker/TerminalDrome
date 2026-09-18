use ratatui::{
    layout::Rect,
    prelude::{Alignment, Frame, Line, Span},
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, Paragraph},
};

pub fn render_help(frame: &mut Frame) {
    let header_style = Style::default()
        .fg(Color::Yellow)
        .add_modifier(Modifier::BOLD);
    let key_style = Style::default().fg(Color::Cyan);

    let help_text = vec![
        Line::from(Span::styled(" TerminalDrome – Keyboard Shortcuts ", header_style)),
        Line::from(""),
        Line::from(Span::styled("▶ Navigation", header_style)),
        Line::from(vec![
            Span::styled("  ↑ / ↓     ", key_style),
            Span::raw("Move selection up / down"),
        ]),
        Line::from(vec![
            Span::styled("  ← / →     ", key_style),
            Span::raw("Switch between views"),
        ]),
        Line::from(vec![
            Span::styled("  Enter     ", key_style),
            Span::raw("Confirm selection / start playback"),
        ]),
        Line::from(vec![
            Span::styled("  Tab       ", key_style),
            Span::raw("Toggle Artists / Playlists"),
        ]),
        Line::from(vec![
            Span::styled("  A–Z       ", key_style),
            Span::raw("Quick jump to first entry"),
        ]),
        Line::from(""),
        Line::from(Span::styled("▶ Playback", header_style)),
        Line::from(vec![
            Span::styled("  Space     ", key_style),
            Span::raw("Stop playback"),
        ]),
        Line::from(vec![
            Span::styled("  n / p     ", key_style),
            Span::raw("Next / previous track"),
        ]),
        Line::from(vec![
            Span::styled("  + / -     ", key_style),
            Span::raw("Volume up / down"),
        ]),
        Line::from(vec![
            Span::styled("  m         ", key_style),
            Span::raw("Toggle mute"),
        ]),
        Line::from(vec![
            Span::styled("  Shift+S   ", key_style),
            Span::raw("Shuffle current list & restart"),
        ]),
        Line::from(vec![
            Span::styled("  Shift+L   ", key_style),
            Span::raw("❤️  Like current song"),
        ]),
        Line::from(""),
        Line::from(Span::styled("▶ Modes", header_style)),
        Line::from(vec![
            Span::styled("  Shift+B   ", key_style),
            Span::raw("Toggle source (Navidrome / Bandcamp)"),
        ]),
        Line::from(vec![
            Span::styled("  Shift+J   ", key_style),
            Span::raw("Start Jukebox / Party Mode"),
        ]),
        Line::from(vec![
            Span::styled("  Shift+E   ", key_style),
            Span::raw("Toggle audio visualizer"),
        ]),
        Line::from(vec![
            Span::styled("  ESC       ", key_style),
            Span::raw("Exit Jukebox / close Visualizer"),
        ]),
        Line::from(""),
        Line::from(Span::styled("▶ Other", header_style)),
        Line::from(vec![
            Span::styled("  /         ", key_style),
            Span::raw("Search"),
        ]),
        Line::from(vec![
            Span::styled("  Shift+H   ", key_style),
            Span::raw("This help screen"),
        ]),
        Line::from(vec![
            Span::styled("  Shift+Q   ", key_style),
            Span::raw("Quit"),
        ]),
    ];

    let sz     = frame.size();
    let height = 32u16.min(sz.height.saturating_sub(2));
    let width  = 62u16.min(sz.width.saturating_sub(4));
    let x      = sz.width.saturating_sub(width) / 2;
    let y      = sz.height.saturating_sub(height) / 2;
    let area   = Rect { x, y, width, height };

    frame.render_widget(
        Paragraph::new(help_text)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" Help ")
                    .border_style(Style::default().fg(Color::LightBlue)),
            )
            .alignment(Alignment::Left),
        area,
    );
}