// src/main.rs 2025-05-08 08:03 Uhr
mod api;
mod audio;
mod config;
mod ui;

use api::NavidromeClient;
use audio::AudioPlayer;
use crossterm::{
    event::{self},
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

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1. Konfiguration laden
    let config = config::AppConfig::load()?;
    
    // 2. Kopien der benötigten Werte erstellen
    let server_url = config.server.url.clone();
    let username = config.server.username.clone();
    let password = config.server.password.clone();

    // 3. Navidrome-Client initialisieren
    let client = NavidromeClient::new(server_url, username, password);

    // 4. Terminal-UI vorbereiten
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // 5. Daten laden mit Debug-Ausgabe
    println!("Fetching artists from server...");
    let artists_with_ids = client.get_artists()?;
    println!("Received {} artists", artists_with_ids.len());
    
    if artists_with_ids.is_empty() {
        eprintln!("Warning: No artists received from server!");
    }

    let artist_names: Vec<String> = artists_with_ids.iter()
        .map(|(name, _)| name.clone())
        .collect();
    let mut ui = UI::new(artist_names);
    let mut player = AudioPlayer::new(&config);

    // 6. Haupt-Event-Loop
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
                        if !artists_with_ids.is_empty() {
                            if let Some((_, id)) = artists_with_ids.iter()
                                .find(|(name, _)| name == &artist_name) 
                            {
                                println!("Attempting to play: {}", artist_name);
                                let stream_url = client.get_stream_url(id);
                                player.play(&stream_url);
                            }
                        }
                    }
                }
            }
        }
    }

    // 7. Aufräumen
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    Ok(())
}