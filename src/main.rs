#[macro_use]
extern crate lazy_static;

use std::{
    error::Error,
    io::{self, Write},
    sync::atomic::Ordering,
    time::{Duration, Instant},
};

use anyhow::Result;
use clap::Parser;
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
mod cli;
mod config;
mod cover;
mod ui;
mod visual;

use api::{check_bandcamp_connection, check_connection};
use api::endpoints::search_songs;
use app::normalize_for_search;
use app::{App, PanelState, ViewMode};
use cli::Cli;
use config::{read_config, setup_initial_credentials};
use ui::ui;

/// The TerminalDrome wordmark.
///
/// This is the single source of truth: the splash screen below embeds it with
/// `include_str!` so it cannot go stale at runtime, and the README quotes the
/// very same file. A unit test fails the build if the two ever drift apart.
const LOGO: &str = include_str!("../docs/logo.txt");

/// Composes the splash screen around [`LOGO`].
///
/// The version line is passed in because it is built from `CARGO_PKG_VERSION`
/// at runtime; everything else is static. All lines are right-padded to a
/// uniform width so the block stays rectangular.
fn splash_lines(version_line: &str) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    lines.push(String::new());
    lines.push("  This is:".to_string());
    lines.extend(LOGO.lines().map(str::to_string));
    lines.push(String::new());
    lines.push(version_line.to_string());
    lines.push("   Made with love   <3   in Mitteldeutschland".to_string());
    lines.push(String::new());

    let width = lines.iter().map(|l| l.chars().count()).max().unwrap_or(54);
    lines.iter().map(|l| format!("{l:width$}")).collect()
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    // 1. Parse CLI arguments (--help, --server, --user, etc.)
    let cli_args = Cli::parse();

    // 1a. `--about` only prints a text and exits. It is handled before the
    // config is read so that it also works on a fresh installation without
    // any credentials.
    if cli_args.about {
        print!("{}", cli::about_text());
        return Ok(());
    }

    // 2. Load configuration and apply CLI overrides
    let mut config = match read_config() {
        Ok(cfg) => cfg,
        Err(e) => {
            eprintln!("Configuration error: {}", e);
            std::process::exit(1);
        }
    };

    if let Some(server_url) = cli_args.server {
        config.server.url = server_url;
    }
    if let Some(username) = cli_args.user {
        config.server.username = username;
    }

    // Check for placeholder values or missing credentials
    if config.server.url.contains("example.com")
        || config.server.username == "your_username"
        || config.server.username.is_empty()
        || (config.server.token.as_ref().map_or(true, |t| t.is_empty())
            && config.server.password.as_ref().map_or(true, |p| p.is_empty()))
    {
        println!("⚠️ No complete or valid configuration found.");
        if let Err(e) = setup_initial_credentials(&mut config) {
            eprintln!("Error during first-time setup: {}", e);
            std::process::exit(1);
        }
    }

    // Pre-Flight Connection Check
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

    // The optional Bandcamp source is verified too, but a failure here is only
    // a warning: Navidrome is the primary source and the user can still fix
    // config.toml. Reporting it now beats discovering it later as an empty
    // collection with no explanation.
    if let Some(result) = check_bandcamp_connection(&config).await {
        match result {
            Ok(()) => println!("Bandcamp connection successful."),
            Err(e) => println!("⚠️  Bandcamp is configured but not usable: {}", e),
        }
    }

    // 3. Set up terminal panic hook
    std::panic::set_hook(Box::new(|panic_info| {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen, DisableMouseCapture);
        eprintln!("Panic occurred: {:?}", panic_info);
    }));

    // 4. Initialize terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // go pgramming, it is a good thing in live they said
    // Splash screen. Die Versionszeile wird aus Cargo.toml erzeugt, damit sie
    // beim Release nicht von Hand veralten kann.
    let version_line = format!("   Version {:<20}by Jan Montag", env!("CARGO_PKG_VERSION"));
    let raw_lines = splash_lines(&version_line);
    let splash_width = raw_lines.iter().map(|l| l.chars().count()).max().unwrap_or(54) as u16;
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
                    // ── Modal priority chain ──────────────────────────────────
                    // Only one modal is active at a time. The order here
                    // matters: help > playlist picker > song info > normal input.
                    if app.is_help_mode {
                        app.is_help_mode = false;
                    } else if app.playlist_picker.is_some() {
                        let creating = app
                            .playlist_picker
                            .as_ref()
                            .map(|p| p.creating)
                            .unwrap_or(false);

                        if creating {
                            // ── New-playlist name entry ────────────────────────
                            match key.code {
                                KeyCode::Esc => {
                                    if let Some(p) = app.playlist_picker.as_mut() {
                                        p.creating = false;
                                        p.new_name.clear();
                                    }
                                }
                                KeyCode::Enter => {
                                    let _ = app.create_playlist_with_song().await;
                                }
                                KeyCode::Backspace => {
                                    if let Some(p) = app.playlist_picker.as_mut() {
                                        p.new_name.pop();
                                    }
                                }
                                KeyCode::Char(c)
                                    if !key.modifiers.contains(KeyModifiers::CONTROL)
                                        && !key.modifiers.contains(KeyModifiers::SHIFT) =>
                                {
                                    if let Some(p) = app.playlist_picker.as_mut() {
                                        p.new_name.push(c);
                                    }
                                }
                                _ => {}
                            }
                        } else {
                            // ── Playlist selection ─────────────────────────────
                            match key.code {
                                KeyCode::Esc => {
                                    app.close_playlist_picker();
                                }
                                KeyCode::Up => {
                                    if let Some(p) = app.playlist_picker.as_mut() {
                                        if p.selected > 0 {
                                            p.selected -= 1;
                                        }
                                    }
                                }
                                KeyCode::Down => {
                                    let len = app.playlists.len();
                                    if let Some(p) = app.playlist_picker.as_mut() {
                                        if p.selected + 1 < len {
                                            p.selected += 1;
                                        }
                                    }
                                }
                                KeyCode::Enter => {
                                    let _ = app.confirm_playlist_pick().await;
                                }
                                KeyCode::Char('N')
                                    if key.modifiers.contains(KeyModifiers::SHIFT) =>
                                {
                                    if let Some(p) = app.playlist_picker.as_mut() {
                                        p.creating = true;
                                        p.new_name.clear();
                                    }
                                }
                                _ => {}
                            }
                        }
                    } else if app.song_info_overlay.is_some() {
                        app.close_song_info();
                    } else {
                        match key.code {
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

                            // ── Playlist management ───────────────────────────
                            // 'a' opens the picker for the current song.
                            KeyCode::Char('a') if !app.is_search_mode => {
                                let _ = app.open_playlist_picker().await;
                            }
                            // 'd' removes the selected song from the open playlist.
                            KeyCode::Char('d')
                                if !app.is_search_mode
                                    && app.mode == ViewMode::PlaylistSongs =>
                            {
                                let _ = app.remove_from_current_playlist().await;
                            }

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

                            // Song info overlay.
                            //  - Shift+I on modern terminals → Char('i') + SHIFT
                            //  - Shift+I on macOS Terminal.app → Char('I') without any modifier flag
                            //  - Ctrl+I on terminals that support CSI-u → Char('i') + CONTROL
                            KeyCode::Char('I') if !app.is_search_mode => {
                                let _ = app.open_song_info().await;
                            }
                            KeyCode::Char('i')
                                if (key.modifiers.contains(KeyModifiers::SHIFT)
                                    || key.modifiers.contains(KeyModifiers::CONTROL))
                                    && !app.is_search_mode =>
                            {
                                let _ = app.open_song_info().await;
                            }

                            KeyCode::Char(c)
                                if c.is_alphabetic()
                                    && !app.is_search_mode
                                    && !key.modifiers.contains(KeyModifiers::SHIFT)
                                    && !key.modifiers.contains(KeyModifiers::CONTROL)
                                    // 'a' and 'd' are reserved in some views, so
                                    // exclude them here to avoid an unintended
                                    // A-Z jump when the specific handlers above
                                    // don't match (e.g. 'd' outside PlaylistSongs).
                                    // 'x' stops playback.
                                    && !matches!(c, 'n' | 'p' | 'm' | 'h' | 'q' | 'a' | 'd' | 'x') =>
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
                            // Space pausiert bzw. setzt fort — wie in mpv.
                            // Die View bleibt dabei stehen, damit man direkt
                            // an derselben Stelle weitermachen kann.
                            KeyCode::Char(' ') if !app.is_search_mode => {
                                app.toggle_pause().await;
                            }
                            // 'x' stoppt endgueltig. Da Space nur pausiert,
                            // braucht es dafuer eine eigene Taste.
                            KeyCode::Char('x') if !app.is_search_mode => {
                                let was_jukebox = app.is_jukebox_mode;
                                app.stop_playback().await;
                                // Im Jukebox-Modus heisst Stop auch "Modus
                                // verlassen" — sonst bliebe die View auf
                                // Jukebox, waehrend der Modus schon aus ist.
                                // In allen anderen Views bleibt die Position
                                // erhalten, damit direkt neu gestartet
                                // werden kann.
                                if was_jukebox {
                                    app.mode = ViewMode::Artists;
                                }
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

#[cfg(test)]
mod splash_tests {
    use super::{LOGO, splash_lines};

    const README: &str = include_str!("../README.md");

    /// The logo file is the single source of truth. Every one of its lines has
    /// to appear verbatim in the README, otherwise the two wordmarks have
    /// drifted apart again.
    #[test]
    fn readme_quotes_the_canonical_wordmark() {
        for line in LOGO.lines() {
            assert!(
                README.contains(line),
                "docs/logo.txt has a line that the README does not contain:\n  {line}\n\
                 Either update the README wordmark or change docs/logo.txt."
            );
        }
    }

    /// The README must not carry a stale second copy of the tagline.
    #[test]
    fn readme_has_no_duplicate_wordmark_tagline() {
        let tagline = LOGO
            .lines()
            .find(|l| l.contains("__ \\  "))
            .expect("logo should carry a tagline in the D of DROME");
        let tagline_text = tagline.trim_start_matches("   |  __ \\  ");
        assert_eq!(
            README.matches(tagline_text).count(),
            1,
            "the tagline {tagline_text:?} should appear exactly once in the README"
        );
    }

    /// All splash lines are padded to one width, otherwise the centred block
    /// gets a ragged right edge.
    #[test]
    fn splash_lines_are_uniformly_widened() {
        let lines = splash_lines("   Version 0.9.5                by Jan Montag");
        let widths: Vec<usize> = lines.iter().map(|l| l.chars().count()).collect();
        assert!(
            widths.windows(2).all(|w| w[0] == w[1]),
            "splash lines have uneven widths: {widths:?}"
        );
    }

    /// The logo must be pure ASCII, because the renderer pads and measures it
    /// in character cells. A non-ASCII glyph would desynchronise the block.
    #[test]
    fn logo_is_ascii_only() {
        for (n, line) in LOGO.lines().enumerate() {
            assert!(
                line.is_ascii(),
                "docs/logo.txt line {} contains a non-ASCII character: {line}",
                n + 1
            );
        }
    }
}
