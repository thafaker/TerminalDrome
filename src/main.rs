mod api;
mod audio;
mod config;
mod ui;

use crate::api::{get_artists, get_songs_by_artist};
use crate::audio::AudioPlayer;
use crate::config::AppConfig;
use crate::ui::{UI, Action};
use anyhow::Result;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::{io, time::Duration};

fn main() -> Result<()> {
    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Load config and setup app state
    let config = AppConfig::load()?;
    let mut player = AudioPlayer::new(&config);
    let artists = get_artists(&config)?;
    let artist_names = artists.iter().map(|a| a.name.clone()).collect();
    let mut ui = UI::new(artist_names);
    let mut current_artist_id = None;

    // Main loop
    loop {
        terminal.draw(|f| ui.draw(f))?;

        if event::poll(Duration::from_millis(100))? {
            if let Some(action) = ui.handle_input(event::read()?) {
                match action {
                    Action::Quit => break,
                    Action::SelectArtist(idx) => {
                        current_artist_id = Some(artists[idx].id.clone());
                        let songs = get_songs_by_artist(&config, &artists[idx].id)?;
                        ui.songs = songs.iter().map(|s| format!("{} - {}", s.artist, s.title)).collect();
                        ui.in_song_view = true;
                        ui.list_state.select(Some(0));
                    }
                    Action::PlaySong(idx) => {
                        if let Some(artist_id) = &current_artist_id {
                            let songs = get_songs_by_artist(&config, artist_id)?;
                            player.play_song(&songs[idx].id);
                        }
                    }
                }
            }
        }
    }

    // Cleanup terminal
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    Ok(())
}
