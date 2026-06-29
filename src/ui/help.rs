use ratatui::{
    layout::Rect,
    prelude::{Alignment, Frame, Line},
    style::{Color, Style},
    widgets::{Block, Borders, Paragraph},
};

pub fn render_help(frame: &mut Frame) {
    let help_text = vec![
        Line::from(" TerminalDrome – Keyboard Shortcuts ").style(Style::default().fg(Color::Yellow)),
        Line::from(""),
        Line::from("▶ Navigation:"),
        Line::from("  ↑/↓      - Move selection"),
        Line::from("  ←/→      - Switch views"),
        Line::from("  Enter    - Confirm selection"),
        Line::from("  Tab      - Toggle Playlists / Artists"),
        Line::from(""),
        Line::from("▶ Playback:"),
        Line::from("  Space    - Stop"),
        Line::from("  n        - Next track"),
        Line::from("  p        - Previous track"),
        Line::from("  +        - Volume up"),
        Line::from("  -        - Volume down"),
        Line::from("  m        - Toggle mute"),
        Line::from("  Shift+S  - Shuffle current playlist/album & restart"),
        Line::from(""),
        Line::from("▶ Jukebox / Party Mode:"),
        Line::from("  Shift+J  - Start Jukebox (shuffles entire library)"),
        Line::from("  ESC      - Stop Jukebox & return to Artists"),
        Line::from(""),
        Line::from("▶ Other:"),
        Line::from("  Shift+L  - Like current song"),
        Line::from("  Shift+E  - Visualizer"),
        Line::from("  /        - Search"),
        Line::from("  A-Z      - Quick jump in lists"),
        Line::from("  Shift+Q  - Quit"),
        Line::from("  Shift+H  - This help screen"),
    ];

    let sz       = frame.size();
    let height   = (29u16).min(sz.height.saturating_sub(2));
    let width    = (sz.width / 2).min(sz.width);
    let x        = sz.width.saturating_sub(width) / 2;
    let y        = sz.height.saturating_sub(height) / 2;
    let area     = Rect { x, y, width, height };

    frame.render_widget(
        Paragraph::new(help_text)
            .block(Block::default().borders(Borders::ALL).title(" Help ").border_style(Style::default().fg(Color::LightBlue)))
            .alignment(Alignment::Left),
        area,
    );
}
