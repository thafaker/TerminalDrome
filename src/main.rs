mod api;
mod audio;
mod config;
mod ui;

use anyhow::{Context, Result};
use api::NavidromeClient;
use audio::AudioPlayer;
use crossterm::{
    event::{self, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    widgets::{Block, Borders, List, ListItem},
    Terminal,
};
use std::{io, time::Duration};
use ui::{Action, UI};

fn main() -> Result<()> {
    // 1. Konfiguration laden
    let config = config::AppConfig::load()
        .context("Failed to load config. Create a config.toml in ~/.config/termnavi/")?;

    // 2. Navidrome-Client initialisieren
    let server_url = config.server.url.clone();
    let username = config.server.username.clone();
    let password = config.server.password.clone();
    let client = NavidromeClient::new(server_url, username, password);

    // 3. Terminal-UI vorbereiten
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // 4. Daten laden
    let artists_with_ids = client.get_artists()?;
    let artist_names: Vec<String> = artists_with_ids.iter()
        .map(|(name, _)| name.clone())
        .collect();
    let mut ui = UI::new(artist_names);
    let mut player = AudioPlayer::new(&config);

    // 5. Haupt-Event-Loop
    loop {
        terminal.draw(|f| {
            let items: Vec<ListItem> = ui
                .artists
                .iter()
                .enumerate()
                .map(|(i, artist)| {
                    let prefix = if i == ui.selected { "> " } else { "  " };
                    ListItem::new(format!("{}{}", prefix, artist))
                })
                .collect();

            let list = List::new(items)
                .block(Block::default().title("Artists").borders(Borders::ALL));
            f.render_widget(list, f.size());
        })?;

        if event::poll(Duration::from_millis(100))? {
            if let Some(action) = ui.handle_input(event::read()?) {
                match action {
                    Action::Quit => break,
                    Action::Play(artist_name) => {
                        if let Some((_, id)) = artists_with_ids.iter()
                            .find(|(name, _)| name == &artist_name) 
                        {
                            let stream_url = client.get_stream_url(id);
                            player.play(&stream_url);
                        }
                    }
                }
            }
        }
    }

    // 6. Aufräumen
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    Ok(())
}