use ratatui::{
    layout::Rect,
    prelude::{Alignment, Frame, Line, Span},
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, Clear, Paragraph},
};

use crate::app::App;

pub fn render_playlist_picker(frame: &mut Frame, app: &App) {
    let Some(picker) = app.playlist_picker.as_ref() else { return };

    let sz     = frame.size();
    let height = (sz.height.saturating_sub(6)).min(20);
    let width  = 64u16.min(sz.width.saturating_sub(4));
    let x      = sz.width.saturating_sub(width) / 2;
    let y      = sz.height.saturating_sub(height) / 2;
    let area   = Rect { x, y, width, height };

    frame.render_widget(Clear, area);

    let head = Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD);
    let dim  = Style::default().fg(Color::DarkGray);
    let cyan = Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD);

    let title = format!(" Add to playlist — {} ", picker.song.title);

    if picker.creating {
        render_create_prompt(frame, area, &title, picker);
    } else {
        render_picker_list(frame, app, area, &title, head, dim, cyan);
    }
}

fn render_picker_list(
    frame: &mut Frame,
    app: &App,
    area: Rect,
    title: &str,
    head: Style,
    dim: Style,
    cyan: Style,
) {
    let Some(picker) = app.playlist_picker.as_ref() else { return };

    // Layout inside the modal:
    //   - 1 line: title
    //   - 1 blank line
    //   - N lines: list of playlists (scrollable)
    //   - 1 blank line
    //   - 1 line: key hints
    let inner_height = area.height.saturating_sub(2) as usize;
    let header_lines = 2usize;
    let footer_lines = 2usize;
    let list_height  = inner_height
        .saturating_sub(header_lines + footer_lines)
        .max(1);

    let mut lines: Vec<Line> = Vec::new();
    lines.push(Line::from(Span::styled(format!(" {}", title), head)));
    lines.push(Line::from(""));

    if app.playlists.is_empty() {
        lines.push(Line::from(Span::styled(
            "  No playlists yet — press Shift+N to create one",
            dim,
        )));
    } else {
        let start = picker.selected.saturating_sub(list_height.saturating_sub(1));
        for (i, pl) in app
            .playlists
            .iter()
            .enumerate()
            .skip(start)
            .take(list_height)
        {
            let is_sel = i == picker.selected;
            let style = if is_sel {
                Style::default().fg(Color::Blue).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Gray)
            };
            let prefix = if is_sel { "▶ " } else { "  " };
            lines.push(Line::from(Span::styled(
                format!("{}{} ({})", prefix, pl.name, pl.song_count),
                style,
            )));
        }
    }

    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled("  ↑↓ ", cyan),
        Span::styled("select", dim),
        Span::styled("   Enter ", cyan),
        Span::styled("confirm", dim),
        Span::styled("   Shift+N ", cyan),
        Span::styled("new", dim),
        Span::styled("   Esc ", cyan),
        Span::styled("cancel", dim),
    ]));

    frame.render_widget(
        Paragraph::new(lines)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" Add to playlist ")
                    .border_style(Style::default().fg(Color::Magenta)),
            )
            .alignment(Alignment::Left),
        area,
    );
}

fn render_create_prompt(
    frame: &mut Frame,
    area: Rect,
    title: &str,
    picker: &crate::app::PlaylistPickerOverlay,
) {
    let dim  = Style::default().fg(Color::DarkGray);
    let head = Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD);
    let cyan = Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD);

    let lines = vec![
        Line::from(Span::styled(format!(" {}", title), head)),
        Line::from(""),
        Line::from(Span::styled("  New playlist name:", dim)),
        Line::from(""),
        Line::from(Span::styled(
            format!("  {}_", picker.new_name),
            Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(vec![
            Span::styled("  Enter ", cyan),
            Span::styled("create with current song   ", dim),
            Span::styled("Esc ", cyan),
            Span::styled("cancel", dim),
        ]),
    ];

    frame.render_widget(
        Paragraph::new(lines)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" Create playlist ")
                    .border_style(Style::default().fg(Color::Magenta)),
            )
            .alignment(Alignment::Left),
        area,
    );
}