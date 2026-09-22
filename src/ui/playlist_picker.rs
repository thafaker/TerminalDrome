use ratatui::{
    layout::Rect,
    prelude::{Alignment, Frame, Line, Span},
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph},
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

    // Top: header line, bottom: hint line, middle: scrollable list.
    let inner_height = area.height.saturating_sub(2) as usize;
    let header_lines = 2usize;
    let footer_lines = 2usize;
    let list_height  = inner_height.saturating_sub(header_lines + footer_lines).max(1);

    let visible: Vec<ListItem> = if app.playlists.is_empty() {
        vec![ListItem::new(Line::from(Span::styled(
            "  No playlists yet — press Shift+N to create one",
            dim,
        )))]
    } else {
        app.playlists
            .iter()
            .enumerate()
            .skip(picker.selected.saturating_sub(list_height.saturating_sub(1)))
            .take(list_height)
            .map(|(i, pl)| {
                let is_sel = i == picker.selected;
                let style = if is_sel {
                    Style::default().fg(Color::Blue).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::Gray)
                };
                let prefix = if is_sel { "▶ " } else { "  " };
                ListItem::new(format!("{}{} ({})", prefix, pl.name, pl.song_count)).style(style)
            })
            .collect()
    };

    // Compose a single Paragraph that holds header + list + footer.
    // We use Paragraph directly (not List inside) so borders and spacing
    // remain predictable on a small terminal.
    let mut lines: Vec<Line> = Vec::new();
    lines.push(Line::from(Span::styled(format!(" {}", title), head)));
    lines.push(Line::from(""));
    for item in &visible {
        // Ratatui ListItem can be rendered via .to_line()? No — we rebuild:
        // we already formatted them as strings above, so reconstruct:
        let _ = item;
    }
    // Rebuild the list items as plain Lines (simpler than nesting List).
    let item_lines: Vec<Line> = if app.playlists.is_empty() {
        vec![Line::from(Span::styled(
            "  No playlists yet — press Shift+N to create one",
            dim,
        ))]
    } else {
        app.playlists
            .iter()
            .enumerate()
            .skip(picker.selected.saturating_sub(list_height.saturating_sub(1)))
            .take(list_height)
            .map(|(i, pl)| {
                let is_sel = i == picker.selected;
                let style = if is_sel {
                    Style::default().fg(Color::Blue).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::Gray)
                };
                let prefix = if is_sel { "▶ " } else { "  " };
                Line::from(Span::styled(
                    format!("{}{} ({})", prefix, pl.name, pl.song_count),
                    style,
                ))
            })
            .collect()
    };
    // Drop the ListItem pass; we only need the Line pass.
    drop(visible);
    lines.extend(item_lines);

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
