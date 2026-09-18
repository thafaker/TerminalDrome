#[macro_use]
extern crate lazy_static;

// Learn to code they said… it will be fun they said!

use std::{
    error::Error,
    io::{self, Write},
    sync::atomic::Ordering,
    time::{Duration, Instant},
};

use anyhow::Result;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::Rect,
    prelude::Alignment,
    style::{Color, Style},
    widgets::Paragraph,
    Terminal,
};

mod api;
mod app;
mod config;
mod cover;
mod ui;
mod visual;

use api::check_connection;
use api::endpoints::search_songs;
use app::normalize_for_search;
use app::{App, PanelState, ViewMode};
use config::{read_config, setup_initial_credentials};
use ui::ui;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    // 1. Konfiguration laden & CLI-Befehle abfangen (--init, --server, etc.)
    let mut config = match read_config() {
        Ok(cfg) => cfg,
        Err(e) => {
            eprintln!("Configuration error: {}", e);
            std::process::exit(1);
        }
    };

    // Prüfen, ob noch Dummy-Werte vorhanden sind oder Daten fehlen
    if config.server.url.contains("example.com")
        || config.server.username == "your_username"
        || config.server.username.is_empty()
        || config.server.password.is_empty()
    {
        println!("⚠️ Keine gültige Konfiguration gefunden.");
        if let Err(e) = setup_initial_credentials(&mut config) {
            eprintln!("Fehler bei der Erstkonfiguration: {}", e);
            std::process::exit(1);
        }
    }

    // Pre-Flight Check
    println!("Connecting to {}...", config.server.url);
    if let Err(e) = check_connection(&config).await {
        eprintln!("\n⚠️ Connection failed: {}", e);

        print!("\nWould you like to set up / update your credentials now? [Y/n]: ");
        io::stdout().flush()?;

        let mut input = String::new();
        io::stdin().read_line(&mut input)?;

        if input.trim().eq_ignore_ascii_case("y") || input.trim().is_empty() {
            if let Err(prompt_err) = setup_initial_credentials(&mut config) {
                eprintln!("Error updating credentials: {}", prompt_err);
                std::process::exit(1);
            }

            println!("Re-connecting with new credentials...");
            if let Err(recheck_err) = check_connection(&config).await {
                eprintln!("\nConnection failed again: {}", recheck_err);
                std::process::exit(1);
            }
        } else {
            std::process::exit(1);
        }
    }
    println!("Connection successful! Starting TerminalDrome...");

    // 3. Terminal-Panic Hook einrichten
    std::panic::set_hook(Box::new(|panic_info| {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen, DisableMouseCapture);
        eprintln!("Panic occurred: {:?}", panic_info);
    }));

    // 4. Terminal initialisieren
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Splash screen
    let raw_lines = vec![
        r"                                                      ",
        r"  This is:                                            ",
        r"    _______                  _             _          ",
        r"   |__   __|                (_)           | |         ",
        r"      | | ___ _ __ _ __ ___  _ _ __   __ _| |        ",
        r"      | |/ _ \ '__| '_ ` _ \| | '_ \ / _` | |       ",
        r"      | |  __/ |  | | | | | | | | | | (_| | |        ",
        r"    __|_|\___|_|  |_| |_| |_|_|_| |_|\__,_|_|       ",
        r"   |  __ \  w. Bandcamp Support                     ",
        r"   | |  | |_ __ ___  _ __ ___   ___                  ",
        r"   | |  | | '__/ _ \| '_ ` _ \ / _ \                ",
        r"   | |__| | | | (_) | | | | | |  __/                 ",
        r"   |_____/|_|  \___/|_| |_| |_|\___|                 ",
        r"                                                     ",
        r"   Version 0.8.5                by Jan Montag        ",
        r"   Made with love   <3   in Mitteldeutschland         ",
        r"                                                     ",
    ];
    let splash_width = raw_lines.iter().map(|l| l.len()).max().unwrap_or(54) as u16;
    let splash_height = raw_lines.len() as u16;
    let splash_text = raw_lines.join("\n");

    terminal.draw(|f| {
        let sz = f.size();
        let area = Rect {
            x: sz.width.saturating_sub(splash_width) / 2,
            y: sz.height.saturating_sub(splash_height) / 2,
            width: splash_width.min(sz.width),
            height: splash_height.min(sz.height),
        };
        f.render_widget(
            Paragraph::new(splash_text.as_str())
                .style(Style::default().fg(Color::LightBlue))
                .alignment(Alignment::Left),
            area,
        );
    })?;
    tokio::time::sleep(Duration::from_secs(2)).await;

    let mut app = App::new().await?;
    app.reset_to_artist_view().await?;

    let mut last_ui_update = Instant::now();
    let ui_refresh_rate = Duration::from_millis(100);

    loop {
        let effective_refresh = if app.mode == ViewMode::Visualizer {
            Duration::from_millis(33)
        } else {
            ui_refresh_rate
        };

        if last_ui_update.elapsed() > effective_refresh {
            app.update_now_playing().await;
            app.check_and_scrobble().await;
            if app.is_jukebox_mode {
                app.jukebox_tick().await?;
            }
            if app.mode == ViewMode::Visualizer {
                app.visualizer.tick();
            }
            terminal.draw(|f| ui(f, &app))?;
            last_ui_update = Instant::now();
        }

        if event::poll(Duration::from_millis(50))? {
            match event::read()? {
                Event::Key(key) if key.kind == KeyEventKind::Press => {
                    if app.is_help_mode {
                        app.is_help_mode = false;
                    } else {
                        match key.code {
                            // 1. DEDIZIERTE SHIFT-BEFEHLE (Immer ganz nach oben!)
                            KeyCode::Char('H') if key.modifiers.contains(KeyModifiers::SHIFT) => {
                                app.is_help_mode = true;
                            }
                            KeyCode::Char('L') if key.modifiers.contains(KeyModifiers::SHIFT) && !app.is_search_mode => {
                                let _ = app.like_current_song().await;
                            }
                            KeyCode::Char('Q') if key.modifiers.contains(KeyModifiers::SHIFT) => {
                                app.stop_playback().await;
                                app.should_quit = true;
                            }
                            KeyCode::Char('J') if key.modifiers.contains(KeyModifiers::SHIFT) => {
                                app.start_jukebox().await?;
                            }
                            KeyCode::Char('B') if key.modifiers.contains(KeyModifiers::SHIFT) && !app.is_search_mode => {
                                let _ = app.toggle_music_source().await;
                            }
                            KeyCode::Char('S') if key.modifiers.contains(KeyModifiers::SHIFT) && !app.is_search_mode => {
                                match app.mode {
                                    ViewMode::Songs | ViewMode::PlaylistSongs | ViewMode::Jukebox => {
                                        app.shuffle_and_restart().await?;
                                    }
                                    _ => {}
                                }
                            }
                            KeyCode::Char('E') if key.modifiers.contains(KeyModifiers::SHIFT) && !app.is_search_mode => {
                                if app.mode != ViewMode::Visualizer {
                                    app.prev_mode = app.mode;
                                    app.mode = ViewMode::Visualizer;
                                    let _ = terminal.clear();
                                    let _ = app.visualizer.try_attach_cava();
                                    if let Some(fifo) = app.visualizer.fifo_path().map(|p| p.to_path_buf()) {
                                        if let Some(idx) = app.now_playing {
                                            if let Some(song) = app.songs.get(idx) {
                                                let url = api::build_stream_url(&song.id, app.active_source, &app.config);
                                                let pos_sec = (app.player_status.current_time.load(Ordering::Relaxed) / 1000) as u64;
                                                app.visualizer.start_ffmpeg_feeder(&url, &fifo, pos_sec);
                                            }
                                        }
                                    }
                                } else {
                                    app.mode = app.prev_mode;
                                    app.visualizer.detach_audio();
                                    let _ = terminal.clear();
                                }
                            }

                            // 2. STEUERUNG & WIEDERGABE (Ohne Shift)
                            KeyCode::Char('+') | KeyCode::Char('=') => app.adjust_volume(5).await,
                            KeyCode::Char('-') => app.adjust_volume(-5).await,
                            KeyCode::Char('m') if !app.is_search_mode => {
                                app.toggle_mute().await;
                            }
                            KeyCode::Char('n') if !app.is_search_mode => app.next_track().await,
                            KeyCode::Char('p') if !app.is_search_mode => app.previous_track().await,
                            KeyCode::Tab if !app.is_search_mode => {
                                if !app.is_jukebox_mode {
                                    match app.mode {
                                        ViewMode::Playlists | ViewMode::PlaylistSongs => {
                                            app.mode = ViewMode::Artists;
                                        }
                                        _ => {
                                            app.mode = ViewMode::Playlists;
                                            app.current_album = None;
                                            app.albums.clear();
                                            app.album_state = PanelState::default();
                                        }
                                    }
                                }
                            }

                            // 3. NORMALE BUCHSTABEN / QUICK JUMP (Geschützt gegen SHIFT)
                            KeyCode::Char(c)
                                if c.is_alphabetic()
                                    && !app.is_search_mode
                                    && !key.modifiers.contains(KeyModifiers::SHIFT)
                                    && !matches!(c, 'n' | 'p' | 'm' | 'h' | 'q') =>
                            {
                                let sc = c.to_ascii_lowercase().to_string();
                                match app.mode {
                                    ViewMode::Artists => {
                                        if let Some(pos) = app.artists.iter().position(|a| normalize_for_search(&a.name).starts_with(&sc)) {
                                            app.artist_state.selected = pos;
                                            app.adjust_scroll();
                                        }
                                    }
                                    ViewMode::Albums => {
                                        if let Some(pos) = app.albums.iter().position(|a| normalize_for_search(&a.name).starts_with(&sc)) {
                                            app.album_state.selected = pos;
                                            app.adjust_scroll();
                                        }
                                    }
                                    ViewMode::Songs | ViewMode::PlaylistSongs | ViewMode::Jukebox | ViewMode::Visualizer => {
                                        if let Some(pos) = app.songs.iter().position(|s| normalize_for_search(&s.title).starts_with(&sc)) {
                                            app.song_state.selected = pos;
                                            app.adjust_scroll();
                                        }
                                    }
                                    ViewMode::Playlists => {
                                        if let Some(pos) = app.playlists.iter().position(|pl| normalize_for_search(&pl.name).starts_with(&sc)) {
                                            app.playlist_state.selected = pos;
                                            app.adjust_playlist_scroll();
                                        }
                                    }
                                }
                            }

                            // 4. SUCHE & NAVIGATION
                            KeyCode::Char('/') => {
                                app.is_search_mode = true;
                                app.search_query.clear();
                            }
                            KeyCode::Esc => {
                                if app.mode == ViewMode::Visualizer {
                                    app.mode = app.prev_mode;
                                    app.visualizer.detach_audio();
                                    let _ = terminal.clear();
                                } else {
                                    app.is_search_mode = false;
                                    if app.is_jukebox_mode {
                                        app.stop_playback().await;
                                        app.mode = ViewMode::Artists;
                                    }
                                }
                            }
                            KeyCode::Enter if app.is_search_mode => {
                                match search_songs(app.active_source, &app.search_query, &app.config).await {
                                    Ok(results) => {
                                        app.search_results = results.clone();
                                        app.songs = results;
                                        if app.songs.is_empty() {
                                            app.status_message = "🔍 No results found".to_string();
                                        } else {
                                            app.search_history.push(app.search_query.clone());
                                            app.status_message = format!("✅ Found {} songs", app.songs.len());
                                        }
                                        app.current_artist = None;
                                        app.current_album = None;
                                        app.artist_state = PanelState::default();
                                        app.album_state = PanelState::default();
                                        app.song_state = PanelState::default();
                                        app.mode = ViewMode::Songs;
                                        app.is_search_mode = false;
                                        app.is_jukebox_mode = false;
                                        app.is_shuffle = false;
                                        app.adjust_scroll();
                                    }
                                    Err(e) => {
                                        app.status_message = format!("❌ Search error: {}", e);
                                        app.songs.clear();
                                        app.search_results.clear();
                                        app.is_search_mode = false;
                                        app.mode = ViewMode::Songs;
                                        app.song_state = PanelState::default();
                                    }
                                }
                            }
                            KeyCode::Char(c) if app.is_search_mode => {
                                app.search_query.push(c);
                            }
                            KeyCode::Backspace if app.is_search_mode => {
                                app.search_query.pop();
                            }
                            KeyCode::Up => app.on_up(),
                            KeyCode::Down => app.on_down(),
                            KeyCode::Left => {
                                if !app.is_jukebox_mode {
                                    app.mode = app.mode.previous();
                                }
                            }
                            KeyCode::Right | KeyCode::Enter => match app.mode {
                                ViewMode::Artists => app.load_albums().await?,
                                ViewMode::Albums => app.load_songs().await?,
                                ViewMode::Songs => app.start_playback().await?,
                                ViewMode::Playlists => app.load_playlist_songs().await?,
                                ViewMode::PlaylistSongs => app.start_playback().await?,
                                ViewMode::Jukebox | ViewMode::Visualizer => {}
                            },
                            KeyCode::Char(' ') => {
                                app.stop_playback().await;
                                app.mode = ViewMode::Artists;
                                app.player_status.force_ui_update.store(true, Ordering::Relaxed);
                            }
                            _ => {}
                        }
                    }
                }
                _ => {}
            }
        }

        if app.should_quit {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen, DisableMouseCapture)?;
    terminal.show_cursor()?;
    Ok(())
}