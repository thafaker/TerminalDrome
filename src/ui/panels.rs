use ratatui::{
    layout::{Constraint, Layout, Rect},
    prelude::{Alignment, Frame, Line, Span},
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, List, ListItem, Paragraph},
};

use crate::app::{App, ViewMode};
use crate::cover::{default_cover_art, get_ascii_cover, COVER_CACHE};

pub fn render_artists_panel(frame: &mut Frame, app: &App, area: Rect) {
    let title = if app.search_results.is_empty() {
        format!(" Artists ({}) ", app.artists.len())
    } else {
        " Search Mode ".to_string()
    };
    let border_color = if !app.search_results.is_empty() { Color::Yellow }
        else if matches!(app.mode, ViewMode::Artists) { Color::Cyan }
        else if app.current_artist.is_some() { Color::LightCyan }
        else { Color::DarkGray };

    let items: Vec<ListItem> = app.artists
        .iter()
        .skip(app.artist_state.scroll)
        .take((area.height as usize).saturating_sub(2))
        .enumerate()
        .map(|(i, artist)| {
            let is_selected = app.artist_state.selected == i + app.artist_state.scroll;
            let style = if is_selected { Style::default().fg(Color::Blue) } else { Style::default().fg(Color::Gray) };
            ListItem::new(artist.name.clone()).style(style)
        })
        .collect();

    frame.render_widget(
        List::new(items).block(Block::default().title(title).borders(Borders::ALL).border_style(Style::default().fg(border_color))),
        area,
    );
}

pub fn render_playlists_panel(frame: &mut Frame, app: &App, area: Rect) {
    let title  = format!(" Playlists ({}) ", app.playlists.len());
    let border = if matches!(app.mode, ViewMode::Playlists | ViewMode::PlaylistSongs) { Color::Cyan }
        else if app.current_playlist.is_some() { Color::LightCyan }
        else { Color::DarkGray };

    let items: Vec<ListItem> = app.playlists
        .iter()
        .skip(app.playlist_state.scroll)
        .take((area.height as usize).saturating_sub(2))
        .enumerate()
        .map(|(i, pl)| {
            let abs    = i + app.playlist_state.scroll;
            let is_sel = app.playlist_state.selected == abs;
            let is_active = app.current_playlist.as_ref().map(|p| p.id.as_str()) == Some(pl.id.as_str());
            let style = if is_active { Style::default().fg(Color::Blue).add_modifier(Modifier::BOLD) }
                else if is_sel { Style::default().fg(Color::Blue) }
                else { Style::default().fg(Color::Gray) };
            ListItem::new(format!("♪ {} ({})", pl.name, pl.song_count)).style(style)
        })
        .collect();

    frame.render_widget(
        List::new(items).block(Block::default().title(title).borders(Borders::ALL).border_style(Style::default().fg(border))),
        area,
    );
}

pub fn render_albums_panel(frame: &mut Frame, app: &App, area: Rect) {
    let chunks = Layout::vertical([Constraint::Length(12), Constraint::Min(3)]).split(area);

    let border_color = if !app.search_results.is_empty() { Color::Yellow }
        else if matches!(app.mode, ViewMode::Albums) { Color::Cyan }
        else if app.current_album.is_some() { Color::LightCyan }
        else { Color::DarkGray };

    // Trigger async cover fetch
    let config         = app.config.clone();
    let selected_album = app.albums.get(app.album_state.selected).cloned();
    tokio::spawn(async move {
        if let Some(album) = selected_album {
            let _ = get_ascii_cover(Some(&album), &config).await;
        }
    });

    let current_cover = if let Some(album) = app.albums.get(app.album_state.selected) {
        COVER_CACHE.lock().unwrap()
            .get(album.cover_art.as_deref().unwrap_or(""))
            .cloned()
            .unwrap_or_else(default_cover_art)
    } else {
        default_cover_art()
    };

    let lines: Vec<&str> = current_cover.lines().collect();
    let total_lines       = lines.len().max(1);
    let colored_ascii: Vec<Line> = lines.into_iter().enumerate().map(|(y, line)| {
        let g     = y as f32 / total_lines as f32;
        let color = Color::Rgb((255.0 * (1.0 - g)) as u8, (255.0 * g) as u8, 128);
        Line::from(Span::styled(line, Style::default().fg(color)))
    }).collect();

    frame.render_widget(
        Paragraph::new(colored_ascii)
            .block(Block::default().title(" Cover Art ").borders(Borders::ALL).border_style(Style::default().fg(Color::Magenta)))
            .alignment(Alignment::Left),
        chunks[0],
    );

    let title = if app.search_results.is_empty() {
        match app.albums.len() { 0 => " Albums ".to_string(), n => format!(" Albums ({}) ", n) }
    } else { " Results ".to_string() };

    let items: Vec<ListItem> = app.albums
        .iter()
        .skip(app.album_state.scroll)
        .take((chunks[1].height as usize).saturating_sub(2))
        .enumerate()
        .map(|(i, album)| {
            let abs       = i + app.album_state.scroll;
            let is_sel    = app.album_state.selected == abs;
            let is_active = app.current_album.as_ref().map(|a| a.id.as_str()) == Some(album.id.as_str());
            let style = if is_active { Style::default().fg(Color::Blue).add_modifier(Modifier::BOLD) }
                else if is_sel { Style::default().fg(Color::Blue) }
                else { Style::default().fg(Color::Gray) };
            ListItem::new(format!("{} ({})", album.name, album.year.unwrap_or(0))).style(style)
        })
        .collect();

    frame.render_widget(
        List::new(items).block(Block::default().title(title).borders(Borders::ALL).border_style(Style::default().fg(border_color))),
        chunks[1],
    );
}

pub fn render_songs_panel(frame: &mut Frame, app: &App, area: Rect) {
    let title = if app.is_jukebox_mode {
        format!(" 🎉 Jukebox Queue ({}) ", app.songs.len())
    } else if app.is_shuffle {
        match app.mode {
            ViewMode::PlaylistSongs =>
                app.current_playlist.as_ref().map(|p| format!(" 🔀 {} ({}) ", p.name, app.songs.len()))
                    .unwrap_or_else(|| " 🔀 Shuffled ".to_string()),
            _ =>
                app.current_album.as_ref().map(|a| format!(" 🔀 {} ({}) ", a.name, app.songs.len()))
                    .unwrap_or_else(|| " 🔀 Shuffled ".to_string()),
        }
    } else if !app.search_results.is_empty() {
        format!(" Search: '{}' ({}) ", app.search_query, app.songs.len())
    } else {
        match app.mode {
            ViewMode::PlaylistSongs =>
                app.current_playlist.as_ref().map(|p| format!(" ♪ {} ({}) ", p.name, app.songs.len()))
                    .unwrap_or_else(|| " Playlist Songs ".to_string()),
            _ =>
                app.current_album.as_ref().map(|a| format!(" {} ({}) ", a.name, app.songs.len()))
                    .unwrap_or_else(|| " Songs ".to_string()),
        }
    };

    let is_active_songs = matches!(app.mode, ViewMode::Songs | ViewMode::PlaylistSongs | ViewMode::Jukebox | ViewMode::Visualizer);
    let border_style = if app.is_jukebox_mode { Style::default().fg(Color::Green) }
        else if app.is_shuffle { Style::default().fg(Color::Magenta) }
        else if !app.search_results.is_empty() { Style::default().fg(Color::Yellow) }
        else if is_active_songs { Style::default().fg(Color::Cyan) }
        else if app.now_playing.is_some() { Style::default().fg(Color::LightCyan) }
        else { Style::default().fg(Color::DarkGray) };

    let items: Vec<ListItem> = app.songs
        .iter()
        .skip(app.song_state.scroll)
        .take((area.height as usize).saturating_sub(2))
        .enumerate()
        .map(|(i, song)| {
            let abs        = i + app.song_state.scroll;
            let is_sel     = app.song_state.selected == abs;
            let is_playing = app.now_playing == Some(abs);
            let style = if is_playing {
                Style::default().fg(if app.is_jukebox_mode { Color::Green } else if app.is_shuffle { Color::Magenta } else { Color::Yellow })
                    .add_modifier(Modifier::BOLD)
            } else if is_sel { Style::default().fg(Color::Blue) }
            else { Style::default().fg(Color::Gray) };

            let mins = song.duration / 60;
            let secs = song.duration % 60;
            let text = match (&song.artist, &song.album) {
                (Some(a), Some(al)) => format!("{} - {} - {:02}:{:02} - {}", a, al, mins, secs, song.title),
                (Some(a), None)     => format!("{} - {:02}:{:02} - {}", a, mins, secs, song.title),
                (None, Some(al))    => format!("{} - {:02}:{:02} - {}", al, mins, secs, song.title),
                _                   => format!("{:02}:{:02} - {}", mins, secs, song.title),
            };
            ListItem::new(text).style(style)
        })
        .collect();

    frame.render_widget(
        List::new(items).block(Block::default().title(title).borders(Borders::ALL).border_style(border_style)),
        area,
    );
}

pub fn render_playlist_context_panel(frame: &mut Frame, app: &App, area: Rect) {
    let chunks = Layout::vertical([Constraint::Length(12), Constraint::Min(3)]).split(area);

    let cover = if let Some(i) = app.now_playing {
        if let Some(song) = app.songs.get(i) {
            if let Some(album_name) = song.album.as_deref() {
                if let Some(album) = app.albums.iter().find(|a| a.name == album_name) {
                    if let Some(cover_id) = album.cover_art.as_deref() {
                        COVER_CACHE.lock().unwrap().get(cover_id).cloned().unwrap_or_else(default_cover_art)
                    } else { default_cover_art() }
                } else { default_cover_art() }
            } else { default_cover_art() }
        } else { default_cover_art() }
    } else { default_cover_art() };

    let title = match app.mode {
        ViewMode::Playlists     => " Playlist ",
        ViewMode::PlaylistSongs => " Now Playing ",
        _                       => " Context ",
    };

    let lines: Vec<&str>  = cover.lines().collect();
    let total_lines        = lines.len().max(1);
    let colored_ascii: Vec<Line> = lines.into_iter().enumerate().map(|(y, line)| {
        let g     = y as f32 / total_lines as f32;
        let color = Color::Rgb((255.0 * (1.0 - g)) as u8, (255.0 * g) as u8, 128);
        Line::from(Span::styled(line, Style::default().fg(color)))
    }).collect();

    frame.render_widget(
        Paragraph::new(colored_ascii)
            .block(Block::default().title(title).borders(Borders::ALL).border_style(Style::default().fg(Color::Magenta)))
            .alignment(Alignment::Left),
        chunks[0],
    );

    let mut info: Vec<Line> = Vec::new();
    if let Some(pl) = app.current_playlist.as_ref() {
        info.push(Line::from(vec![
            Span::styled("Playlist: ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            Span::raw(pl.name.clone()),
        ]));
        if let Some(comment) = pl.comment.as_deref() {
            if !comment.trim().is_empty() {
                info.push(Line::from(vec![
                    Span::styled("Note: ", Style::default().fg(Color::DarkGray)),
                    Span::raw(comment.to_string()),
                ]));
            }
        }
        info.push(Line::from(vec![
            Span::styled("Tracks: ", Style::default().fg(Color::DarkGray)),
            Span::raw(format!("{}", app.songs.len())),
        ]));
    } else {
        info.push(Line::from(Span::styled("Select a playlist…", Style::default().fg(Color::DarkGray))));
    }
    if let Some(i) = app.now_playing {
        if let Some(song) = app.songs.get(i) {
            info.push(Line::from(""));
            info.push(Line::from(vec![
                Span::styled("Now: ", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                Span::raw(format!("{} – {}", song.artist.as_deref().unwrap_or("Unknown"), song.title)),
            ]));
        }
    }

    frame.render_widget(
        Paragraph::new(info)
            .block(Block::default().title(" Info ").borders(Borders::ALL).border_style(Style::default().fg(Color::DarkGray)))
            .alignment(Alignment::Left),
        chunks[1],
    );
}
