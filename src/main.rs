use anyhow::Result;
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
#[derive(Debug, Deserialize, Clone)]
struct Config {
    server: ServerConfig,
}

#[derive(Debug, Deserialize, Clone)]
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
    #[serde(flatten)]
    content: ContentType,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum ContentType {
    Artists { artists: ArtistList },
    Albums { artist: ArtistDetail },
    Songs { album: AlbumDetail },
    Directory(MusicDirectory),
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

#[derive(Debug, Deserialize, Clone)]
struct Artist {
    id: String,
    name: String,
}

#[derive(Debug, Deserialize)]
struct ArtistDetail {
    album: Vec<Album>,
}

#[derive(Debug, Deserialize, Clone)]
struct Album {
    id: String,
    name: String,
    artist: String,
    year: Option<i32>,
    songCount: u32,
}

#[derive(Debug, Deserialize)]
struct AlbumDetail {
    song: Vec<Song>,
}

#[derive(Debug, Deserialize, Clone)]
struct Song {
    id: String,
    title: String,
    duration: u64,
    track: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct MusicDirectory {
    child: Vec<Song>,
}

// --- App-Zustand ---
#[derive(Debug, PartialEq, Clone, Copy)]
enum ViewMode {
    Artists,
    Albums,
    Songs,
}

#[derive(Debug, Default)]
struct PanelState {
    selected: usize,
    scroll: usize,
}

struct App {
    artists: Vec<Artist>,
    albums: Vec<Album>,
    songs: Vec<Song>,
    mode: ViewMode,
    should_quit: bool,
    current_player: Option<Child>,
    status_message: String,
    current_artist: Option<Artist>,
    current_album: Option<Album>,
    artist_state: PanelState,
    album_state: PanelState,
    song_state: PanelState,
    now_playing: Option<usize>, // Index des aktuell spielenden Songs
}

impl App {
    async fn new() -> Result<Self> {
        let config = read_config()?;
        let artists = get_artists(&config).await?;
        Ok(Self {
            artists,
            albums: Vec::new(),
            songs: Vec::new(),
            mode: ViewMode::Artists,
            should_quit: false,
            current_player: None,
            status_message: String::new(),
            current_artist: None,
            current_album: None,
            artist_state: PanelState::default(),
            album_state: PanelState::default(),
            song_state: PanelState::default(),
            now_playing: None,
        })
    }

    fn current_state_mut(&mut self) -> &mut PanelState {
        match self.mode {
            ViewMode::Artists => &mut self.artist_state,
            ViewMode::Albums => &mut self.album_state,
            ViewMode::Songs => &mut self.song_state,
        }
    }

    fn on_up(&mut self) {
        let state = self.current_state_mut();
        if state.selected > 0 {
            state.selected -= 1;
        }
        self.adjust_scroll();
    }

    fn on_down(&mut self) {
        let max_index = match self.mode {
            ViewMode::Artists => self.artists.len().saturating_sub(1),
            ViewMode::Albums => self.albums.len().saturating_sub(1),
            ViewMode::Songs => self.songs.len().saturating_sub(1),
        };
        
        let state = self.current_state_mut();
        if state.selected < max_index {
            state.selected += 1;
        }
        self.adjust_scroll();
    }

    fn adjust_scroll(&mut self) {
        let state = self.current_state_mut();
        let visible_items = 15;
        if state.selected < state.scroll {
            state.scroll = state.selected;
        } else if state.selected >= state.scroll + visible_items {
            state.scroll = state.selected - visible_items + 1;
        }
    }

    async fn load_albums(&mut self, config: &Config) -> Result<()> {
        if let Some(artist) = self.artists.get(self.artist_state.selected) {
            self.albums = get_artist_albums(&artist.id, config).await?;
            self.current_artist = Some(artist.clone());
            self.mode = ViewMode::Albums;
        }
        Ok(())
    }

    async fn load_songs(&mut self, config: &Config) -> Result<()> {
        if let Some(album) = self.albums.get(self.album_state.selected) {
            self.songs = get_album_songs(&album.id, config).await?;
            self.current_album = Some(album.clone());
            self.mode = ViewMode::Songs;
        }
        Ok(())
    }

    fn stop_playback(&mut self) {
        if let Some(mut player) = self.current_player.take() {
            let _ = player.kill();
            self.status_message = "Playback stopped".to_string();
            self.now_playing = None;
        }
    }

    fn start_playback(&mut self, config: &Config) {
        self.stop_playback();

        // Setze den aktuell spielenden Song auf den ausgewählten
        self.now_playing = Some(self.song_state.selected);

        // URLs für alle Songs des Albums erstellen
        let urls: Vec<String> = self.songs.iter().map(|song| {
            format!(
                "{}/rest/stream?id={}&u={}&p={}&v=1.16.1&c=termnavi&f=json",
                config.server.url, 
                song.id, 
                config.server.username, 
                config.server.password
            )
        }).collect();

        // MPV-Kommando mit allen URLs erstellen
        let mut command = Command::new("mpv");
        command
            .arg("--no-video")
            .arg("--really-quiet")
            .arg("--no-terminal")
            .arg("--audio-display=no")
            .arg("--msg-level=all=error");

        for url in urls {
            command.arg(url);
        }

        match command.spawn() {
            Ok(child) => {
                self.current_player = Some(child);
                let album_name = self.current_album.as_ref().map(|a| a.name.clone()).unwrap_or_default();
                self.status_message = format!("Playing album: {}", album_name);
            },
            Err(e) => {
                self.status_message = format!("Playback error: {}", e);
            }
        }
    }

    fn get_now_playing_info(&self) -> String {
        if let Some(index) = self.now_playing {
            if let Some(song) = self.songs.get(index) {
                let minutes = song.duration / 60;
                let seconds = song.duration % 60;
                let album = self.current_album.as_ref().map(|a| a.name.clone()).unwrap_or_default();
                let artist = self.current_artist.as_ref().map(|a| a.name.clone()).unwrap_or_default();
                return format!("Now playing: {} - {} - {} ({:02}:{:02})", 
                    artist, album, song.title, minutes, seconds);
            }
        }
        "No song playing".to_string()
    }
}

// --- Hauptfunktion ---
#[tokio::main]
async fn main() -> Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new().await?;

    while !app.should_quit {
        terminal.draw(|f| ui(f, &app))?;
        handle_events(&mut app).await?;
    }

    disable_raw_mode()?;
    execute!(io::stdout(), LeaveAlternateScreen)?;
    Ok(())
}

// --- UI-Rendering ---
fn ui(frame: &mut Frame, app: &App) {
    let main_chunks = Layout::vertical([
        Constraint::Min(3),
        Constraint::Length(3),  // Größeres Status-Panel
    ]).split(frame.size());

    let panels = Layout::horizontal([
        Constraint::Ratio(1, 3),
        Constraint::Ratio(1, 3),
        Constraint::Ratio(1, 3),
    ]).split(main_chunks[0]);

    render_artists_panel(frame, app, panels[0]);
    render_albums_panel(frame, app, panels[1]);
    render_songs_panel(frame, app, panels[2]);

    // Status Panel mit mehr Informationen
    let status_chunks = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
    ]).split(main_chunks[1]);

    let status = Paragraph::new(app.status_message.clone())
        .style(Style::default().fg(Color::Black).bg(Color::DarkGray))
        .block(Block::default().borders(Borders::TOP));
    frame.render_widget(status, status_chunks[0]);

    let now_playing = Paragraph::new(app.get_now_playing_info())
        .style(Style::default().fg(Color::White).bg(Color::DarkGray))
        .block(Block::default());
    frame.render_widget(now_playing, status_chunks[1]);
}

fn render_artists_panel(frame: &mut Frame, app: &App, area: Rect) {
    let title = format!(" Artists ({}) ", app.artists.len());
    let border_style = if app.mode == ViewMode::Artists {
        Style::default().fg(Color::LightCyan)
    } else {
        Style::default().fg(Color::Gray)
    };

    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(border_style);

    let items: Vec<ListItem> = app
        .artists
        .iter()
        .skip(app.artist_state.scroll)
        .take(area.height as usize - 2)
        .enumerate()
        .map(|(i, artist)| {
            let absolute_index = i + app.artist_state.scroll;
            let is_selected = absolute_index == app.artist_state.selected 
                && app.mode == ViewMode::Artists;
            
            let style = if is_selected {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::LightCyan)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Gray)
            };

            ListItem::new(artist.name.clone()).style(style)
        })
        .collect();

    let list = List::new(items)
        .block(block)
        .highlight_symbol("▶ ");

    frame.render_widget(list, area);
}

fn render_albums_panel(frame: &mut Frame, app: &App, area: Rect) {
    let title = match &app.current_artist {
        Some(artist) => format!(" {}'s Albums ({}) ", artist.name, app.albums.len()),
        None => " Albums ".to_string(),
    };

    let border_style = if app.mode == ViewMode::Albums {
        Style::default().fg(Color::LightCyan)
    } else {
        Style::default().fg(Color::Gray)
    };

    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(border_style);

    let items: Vec<ListItem> = app
        .albums
        .iter()
        .skip(app.album_state.scroll)
        .take(area.height as usize - 2)
        .enumerate()
        .map(|(i, album)| {
            let absolute_index = i + app.album_state.scroll;
            let is_selected = absolute_index == app.album_state.selected 
                && app.mode == ViewMode::Albums;
            
            let style = if is_selected {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::LightCyan)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Gray)
            };

            let year = album.year.map(|y| y.to_string()).unwrap_or_default();
            let text = format!("{} ({}) - {} tracks", album.name, year, album.songCount);
            ListItem::new(text).style(style)
        })
        .collect();

    let list = List::new(items)
        .block(block)
        .highlight_symbol("▶ ");

    frame.render_widget(list, area);
}

fn render_songs_panel(frame: &mut Frame, app: &App, area: Rect) {
    let title = match &app.current_album {
        Some(album) => format!(" {} ({}) ", album.name, app.songs.len()),
        None => " Songs ".to_string(),
    };

    let border_style = if app.mode == ViewMode::Songs {
        Style::default().fg(Color::LightCyan)
    } else {
        Style::default().fg(Color::Gray)
    };

    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(border_style);

    let items: Vec<ListItem> = app
        .songs
        .iter()
        .skip(app.song_state.scroll)
        .take(area.height as usize - 2)
        .enumerate()
        .map(|(i, song)| {
            let absolute_index = i + app.song_state.scroll;
            let is_selected = absolute_index == app.song_state.selected 
                && app.mode == ViewMode::Songs;
            let is_playing = Some(absolute_index) == app.now_playing;
            
            let style = if is_playing {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
            } else if is_selected {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::LightCyan)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Gray)
            };

            let minutes = song.duration / 60;
            let seconds = song.duration % 60;
            let text = format!("{:02}:{:02} - {}", minutes, seconds, song.title);
            ListItem::new(text).style(style)
        })
        .collect();

    let list = List::new(items)
        .block(block)
        .highlight_symbol("▶ ");

    frame.render_widget(list, area);
}

// --- Event-Handling ---
async fn handle_events(app: &mut App) -> Result<()> {
    if event::poll(Duration::from_millis(100))? {
        if let Event::Key(key) = event::read()? {
            if key.kind == KeyEventKind::Press {
                match key.code {
                    KeyCode::Char('q') => app.should_quit = true,
                    KeyCode::Up => app.on_up(),
                    KeyCode::Down => app.on_down(),
                    KeyCode::Left => {
                        app.mode = match app.mode {
                            ViewMode::Albums => ViewMode::Artists,
                            ViewMode::Songs => ViewMode::Albums,
                            _ => app.mode,
                        };
                    },
                    KeyCode::Right | KeyCode::Enter => {
                        let config = read_config()?;
                        match app.mode {
                            ViewMode::Artists => app.load_albums(&config).await?,
                            ViewMode::Albums => app.load_songs(&config).await?,
                            ViewMode::Songs => {
                                app.start_playback(&config);
                            }
                        }
                    },
                    KeyCode::Char(' ') => app.stop_playback(),
                    _ => {}
                }
            }
        }
    }
    Ok(())
}

// --- API-Funktionen ---
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
        .send()
        .await?;

    let body = response.text().await?;
    let parsed: SubsonicResponse = serde_json::from_str(&body)?;

    match parsed.response.content {
        ContentType::Artists { artists } => Ok(artists.index.into_iter().flat_map(|g| g.artist).collect()),
        _ => anyhow::bail!("Unexpected response format"),
    }
}

async fn get_artist_albums(artist_id: &str, config: &Config) -> Result<Vec<Album>> {
    let client = reqwest::Client::new();
    let response = client
        .get(format!("{}/rest/getArtist", config.server.url))
        .query(&[
            ("u", config.server.username.as_str()),
            ("p", config.server.password.as_str()),
            ("v", "1.16.1"),
            ("c", "termnavi"),
            ("f", "json"),
            ("id", artist_id),
        ])
        .send()
        .await?;

    let body = response.text().await?;
    let parsed: SubsonicResponse = serde_json::from_str(&body)?;

    match parsed.response.content {
        ContentType::Albums { artist } => Ok(artist.album),
        _ => anyhow::bail!("Unexpected response format"),
    }
}

async fn get_album_songs(album_id: &str, config: &Config) -> Result<Vec<Song>> {
    let client = reqwest::Client::new();
    let response = client
        .get(format!("{}/rest/getAlbum", config.server.url))
        .query(&[
            ("u", config.server.username.as_str()),
            ("p", config.server.password.as_str()),
            ("v", "1.16.1"),
            ("c", "termnavi"),
            ("f", "json"),
            ("id", album_id),
        ])
        .send()
        .await?;

    let body = response.text().await?;
    let parsed: SubsonicResponse = serde_json::from_str(&body)?;

    match parsed.response.content {
        ContentType::Songs { album } => Ok(album.song),
        _ => anyhow::bail!("Unexpected response format"),
    }
}

// --- Konfigurationsfunktionen ---
fn read_config() -> Result<Config> {
    let config = std::fs::read_to_string("config.toml")?;
    Ok(toml::from_str(&config)?)
}