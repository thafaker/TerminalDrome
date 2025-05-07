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

    // 2. Navidrome-Client initialisieren
    let client = NavidromeClient::new(
        config.server.url,
        config.server.username,
        config.server.password,
    );

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