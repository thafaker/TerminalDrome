use anyhow::{Context, Result};
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    prelude::*,
    widgets::{Block, Borders, List, ListItem, Paragraph},
};
use serde::Deserialize;
use std::{
    io,
    process::{Child, Command},
    time::Duration,
};

// --- Konfigurationsstrukturen ---
#[derive(Debug, Deserialize)]
struct Config {
    server: ServerConfig,
}

#[derive(Debug, Deserialize)]
struct ServerConfig {
    url: String,
    username: String,
    password: String,
}

// --- API Response Strukturen ---
#[derive(Debug, Deserialize)]
struct SubsonicResponse {
    #[serde(rename = "subsonic-response")]
    response: SubsonicContent,
}

#[derive(Debug, Deserialize)]
struct SubsonicContent {
    artists: ArtistList,
    directory: Option<MusicDirectory>,
}

#[derive(Debug, Deserialize)]
struct ArtistList {
    index: Vec<ArtistGroup>,
}

#[derive(Debug, Deserialize)]
struct ArtistGroup {
    #[serde(default)]
    artist: Vec<Artist>,
}

#[derive(Debug, Deserialize)]
struct Artist {
    id: String,
    name: String,
}

#[derive(Debug, Deserialize)]
struct MusicDirectory {
    child: Vec<Song>,
}

#[derive(Debug, Deserialize)]
struct Song {
    id: String,
    title: String,
    artist: String,
    duration: u64,
    isDir: bool,
    path: String,
}

// --- App-Zustand ---
struct App {
    artists: Vec<Artist>,
    selected_index: usize,
    scroll: usize,
    should_quit: bool,
    current_player: Option<Child>,
    status_message: String,
}

impl App {
    async fn new() -> Result<Self> {
        let config = read_config()?;
        let artists = get_artists(&config).await?;
        Ok(Self {
            artists,
            selected_index: 0,
            scroll: 0,
            should_quit: false,
            current_player: None,
            status_message: String::new(),
        })
    }

    fn on_up(&mut self) {
        if self.selected_index > 0 {
            self.selected_index -= 1;
            self.adjust_scroll();
        }
    }

    fn on_down(&mut self) {
        if self.selected_index < self.artists.len().saturating_sub(1) {
            self.selected_index += 1;
            self.adjust_scroll();
        }
    }

    fn adjust_scroll(&mut self) {
        let visible_items = 15; // Temporärer fester Wert
        if self.selected_index < self.scroll {
            self.scroll = self.selected_index;
        } else if self.selected_index >= self.scroll + visible_items {
            self.scroll = self.selected_index - visible_items + 1;
        }
    }
}

// --- Hauptfunktion ---
#[tokio::main]
async fn main() -> Result<()> {
    // Terminal Setup
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // App initialisieren
    let mut app = App::new().await?;

    // Haupt-Event-Loop
    while !app.should_quit {
        terminal.draw(|f| ui(f, &app))?;
        handle_events(&mut app)?;
    }

    // Cleanup
    disable_raw_mode()?;
    execute!(io::stdout(), LeaveAlternateScreen)?;
    Ok(())
}

// --- UI-Rendering ---
fn ui(frame: &mut Frame, app: &App) {
    let main_area = Layout::vertical([
        Constraint::Min(1),
        Constraint::Length(1),
    ]).split(frame.size());

    // Artist-Liste
    let main_block = Block::default()
        .title("Navidrome Client - Artists")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::LightCyan));

    let visible_height = main_area[0].height as usize - 2;
    let items: Vec<ListItem> = app
        .artists
        .iter()
        .skip(app.scroll)
        .take(visible_height)
        .enumerate()
        .map(|(i, artist)| {
            let absolute_index = i + app.scroll;
            let is_selected = absolute_index == app.selected_index;
            
            let style = if is_selected {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::LightCyan)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Gray)
            };

            let content = format!("{:>3}. {}", absolute_index + 1, artist.name);
            ListItem::new(content).style(style)
        })
        .collect();

    let list = List::new(items)
        .block(main_block)
        .highlight_symbol("▶ ")
        .highlight_style(Style::default().fg(Color::Yellow));

    frame.render_widget(list, main_area[0]);

    // Statuszeile
    let status = Paragraph::new(app.status_message.clone())
        .style(Style::default().fg(Color::Black).bg(Color::DarkGray))
        .block(Block::default());
    
    frame.render_widget(status, main_area[1]);
}

// --- Event-Handling ---
fn handle_events(app: &mut App) -> Result<()> {
    if event::poll(Duration::from_millis(100))? {
        if let Event::Key(key) = event::read()? {
            if key.kind == KeyEventKind::Press {
                match key.code {
                    KeyCode::Char('q') => app.should_quit = true,
                    KeyCode::Up => app.on_up(),
                    KeyCode::Down => app.on_down(),
                    KeyCode::PageUp => {
                        app.selected_index = app.selected_index.saturating_sub(10);
                        app.adjust_scroll();
                    },
                    KeyCode::PageDown => {
                        app.selected_index = app.selected_index.saturating_add(10).min(app.artists.len() - 1);
                        app.adjust_scroll();
                    },
                    KeyCode::Home => {
                        app.selected_index = 0;
                        app.scroll = 0;
                    },
                    KeyCode::End => {
                        app.selected_index = app.artists.len().saturating_sub(1);
                        app.adjust_scroll();
                    },
                    KeyCode::Enter => {
                        if let Some(artist) = app.artists.get(app.selected_index) {
                            if let Ok(config) = read_config() {
                                app.status_message = format!("Playing {}...", artist.name);
                                play_artist(artist, &config);
                            }
                        }
                    },
                    KeyCode::Char(' ') => {
                        if let Some(player) = &mut app.current_player {
                            let _ = player.kill();
                            app.current_player = None;
                            app.status_message = "Playback stopped".to_string();
                        }
                    },
                    _ => {}
                }
            }
        }
    }
    Ok(())
}

// --- Musikwiedergabe ---
// Änderung in der play_artist Funktion:
fn play_artist(artist: &Artist, config: &Config) {
    // Clone der notwendigen Werte
    let artist_id = artist.id.clone();
    let server_url = config.server.url.clone();
    let username = config.server.username.clone();
    let password = config.server.password.clone();
    
    tokio::task::spawn(async move {
        if let Ok(songs) = get_artist_songs(&artist_id, &server_url, &username, &password).await {
            if let Some(first_song) = songs.first() {
                let url = format!(
                    "{}/rest/stream?id={}&u={}&p={}",
                    server_url, 
                    first_song.id, 
                    username, 
                    password
                );

                let _ = Command::new("mpv")
                    .arg("--no-video")
                    .arg("--quiet")
                    .arg(&url)
                    .spawn()
                    .expect("Failed to start MPV");
            }
        }
    });
}

async fn get_artist_songs(
    artist_id: &str,
    server_url: &str,
    username: &str,
    password: &str,
) -> Result<Vec<Song>> {
    let client = reqwest::Client::new();
    
    let response = client
        .get(format!("{}/rest/getMusicDirectory", server_url))
        .query(&[
            ("u", username),
            ("p", password),
            ("v", "1.16.1"),
            ("c", "termnavi"),
            ("f", "json"),
            ("id", artist_id),
        ])
        .send()
        .await
        .context("API request failed")?;

    let body = response.text().await.context("Failed to read response")?;
    let parsed: SubsonicResponse = serde_json::from_str(&body)
        .context("Failed to parse JSON response")?;

    Ok(parsed.response.directory
        .map(|d| d.child.into_iter().filter(|s| !s.isDir).collect())
        .unwrap_or_default())
}

// --- Konfigurationsfunktionen ---
fn read_config() -> Result<Config> {
    let config = std::fs::read_to_string("config.toml")?;
    Ok(toml::from_str(&config)?)
}

// --- Artist-Liste abrufen ---
async fn get_artists(config: &Config) -> Result<Vec<Artist>> {
    let client = reqwest::Client::new();
    
    let response = client
        .get(format!("{}/rest/getArtists", config.server.url))
        .query(&[
            ("u", config.server.username.as_str()),
            ("p", config.server.password.as_str()),
            ("v", "1.16.1"),
            ("c", "termnavi"),
            ("f", "json"),
        ])
        .timeout(Duration::from_secs(5))
        .send()
        .await
        .context("API request failed")?;

    let body = response.text().await.context("Failed to read response")?;
    let parsed: SubsonicResponse = serde_json::from_str(&body)
        .context("Failed to parse JSON response")?;

    Ok(parsed.response.artists.index
        .into_iter()
        .flat_map(|i| i.artist)
        .collect())
}