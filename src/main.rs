use ratatui::style::Color;
use std::sync::atomic::{AtomicUsize, AtomicU64, AtomicBool, Ordering};
use std::sync::Arc;
use std::{
    error::Error,
    fs,
    io,
    path::Path,
    process::{Child, Command},
    time::{Duration, Instant},
};

use anyhow::Result;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Layout, Rect},
    style::{Modifier, Style},
    widgets::{Block, Borders, List, ListItem, Paragraph},
    Frame, Terminal,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tempfile;
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::UnixStream,
};

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

#[derive(Debug, Deserialize, Clone, Serialize)]
struct Artist {
    id: String,
    name: String,
}

#[derive(Debug, Deserialize)]
struct ArtistDetail {
    album: Vec<Album>,
}

#[derive(Debug, Deserialize, Clone, Serialize)]
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

#[derive(Debug, Deserialize, Clone, Serialize)]
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

#[derive(Debug, PartialEq, Clone, Copy, Serialize, Deserialize)]
enum ViewMode {
    Artists,
    Albums,
    Songs,
}

#[derive(Debug, Default, Serialize, Deserialize, Clone, Copy)]
struct PanelState {
    selected: usize,
    scroll: usize,
}

#[derive(Debug, Serialize, Deserialize)]
struct AppState {
    mode: ViewMode,
    artist_state: PanelState,
    album_state: PanelState,
    song_state: PanelState,
    current_artist: Option<Artist>,
    current_album: Option<Album>,
    now_playing: Option<usize>,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            mode: ViewMode::Artists,
            artist_state: PanelState::default(),
            album_state: PanelState::default(),
            song_state: PanelState::default(),
            current_artist: None,
            current_album: None,
            now_playing: None,
        }
    }
}

#[derive(Default)]
struct PlayerStatus {
    current_index: AtomicUsize,   // Zeile 154
    current_time: AtomicU64,      // Zeile 155
    force_ui_update: AtomicBool,  // Zeile 156 (einziges Vorkommen)
    should_quit: AtomicBool,      // Zeile 157
    songs: AtomicUsize,           // Zeile 158
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
    now_playing: Option<usize>,
    player_status: Arc<PlayerStatus>,
    temp_dir: Option<tempfile::TempDir>,
}

impl Drop for App {
    fn drop(&mut self) {
        if let Some(mut player) = self.current_player.take() {
            let _ = player.kill();
        }
        if let Some(temp_dir) = &self.temp_dir {
            let _ = fs::remove_dir_all(temp_dir.path());
        }
    }
}

impl App {
    async fn new() -> Result<Self> {
        let config = read_config()?;
        let artists = get_artists(&config).await?;
        
        let loaded_state = Self::load_state().unwrap_or_default();
        
        let mut app = Self {
            artists,
            albums: Vec::new(),
            songs: Vec::new(),
            mode: loaded_state.mode,
            should_quit: false,
            current_player: None,
            status_message: String::new(),
            current_artist: loaded_state.current_artist.clone(),
            current_album: loaded_state.current_album.clone(),
            artist_state: loaded_state.artist_state,
            album_state: loaded_state.album_state,
            song_state: loaded_state.song_state,
            now_playing: loaded_state.now_playing,
            player_status: Arc::new(PlayerStatus {
                current_index: AtomicUsize::new(usize::MAX),
                current_time: AtomicU64::new(0),
                force_ui_update: AtomicBool::new(false),
                should_quit: AtomicBool::new(false),
                songs: AtomicUsize::new(0),
            }),
            temp_dir: None,
        };

        if let Some(artist) = &app.current_artist {
            app.albums = get_artist_albums(&artist.id, &config).await?;
        }
        if let Some(album) = &app.current_album {
            app.songs = get_album_songs(&album.id, &config).await?;
        }

        Ok(app)
    }

    fn save_state(&self) -> Result<()> {
        let state = AppState {
            mode: self.mode,
            artist_state: self.artist_state,
            album_state: self.album_state,
            song_state: self.song_state,
            current_artist: self.current_artist.clone(),
            current_album: self.current_album.clone(),
            now_playing: self.now_playing,
        };

        let state_json = serde_json::to_string(&state)?;
        fs::write("state.json", state_json)?;
        Ok(())
    }

    fn load_state() -> Result<AppState> {
        if Path::new("state.json").exists() {
            let state_json = fs::read_to_string("state.json")?;
            Ok(serde_json::from_str(&state_json)?)
        } else {
            Ok(AppState::default())
        }
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
        self.albums.clear();
        self.current_album = None;
        self.songs.clear();
        self.now_playing = None;
        
        if let Some(artist) = self.artists.get(self.artist_state.selected) {
            self.albums = get_artist_albums(&artist.id, config).await?;
            self.current_artist = Some(artist.clone());
            self.mode = ViewMode::Albums;
        }
        Ok(())
    }

    async fn load_songs(&mut self, config: &Config) -> Result<()> {
        self.songs.clear();
        self.now_playing = None;
        
        if let Some(album) = self.albums.get(self.album_state.selected) {
            self.songs = get_album_songs(&album.id, config).await?;
            self.current_album = Some(album.clone());
            self.mode = ViewMode::Songs;
            
            self.song_state.selected = 0;
            self.adjust_scroll();
            
            self.start_playback(config).await?;
        }
        Ok(())
    }

    async fn start_playback(&mut self, config: &Config) -> Result<()> {
        if let Some(mut player) = self.current_player.take() {
            let _ = player.kill();
        }

        let start_index = self.song_state.selected;
        let status = self.player_status.clone();
        let temp_dir = tempfile::tempdir()?;
        let socket_path = temp_dir.path().join("mpv.sock");
        let socket_path_str = socket_path.to_str().unwrap();

        // Validate and clamp start index
        let start_index = self.song_state.selected.clamp(0, self.songs.len().saturating_sub(1));
        println!("Starting playback at index: {} ({} songs)", start_index, self.songs.len());
        
        // Store songs count in atomic
        self.player_status.songs.store(self.songs.len(), Ordering::Release);

        // Reset player status
        self.player_status.current_index.store(usize::MAX, Ordering::Release);

        self.temp_dir = Some(temp_dir);
        self.player_status.force_ui_update.store(true, Ordering::Release);  // UI-Update erzwingen
        self.now_playing = Some(start_index);

        let mut command = Command::new("mpv");
        command
            .arg("--no-video")
            .arg(format!("--playlist-start={}", start_index))
            .arg("--really-quiet")
            .arg("--no-terminal")
            .arg("--audio-display=no")
            .arg("--loop-playlist=no")  // Explicitly disable playlist looping
            .arg("--msg-level=all=error")
            .arg(format!("--input-ipc-server={}", socket_path_str));

        for song in &self.songs {
            let url = format!(
                "{}/rest/stream?id={}&u={}&p={}&v=1.16.1&c=TerminalDrome&f=json",
                config.server.url, 
                song.id, 
                config.server.username, 
                config.server.password
            );
            command.arg(url);
        }

        match command.spawn() {
            Ok(child) => {
                self.current_player = Some(child);
                let album_name = self.current_album.as_ref().map(|a| a.name.as_str()).unwrap_or("");
                self.status_message = format!("Playing: {}", album_name);
                
                let status_clone = status.clone();
                let socket_path_clone = socket_path_str.to_string();
                
                tokio::spawn(async move {
                    loop {
                        match UnixStream::connect(&socket_path_clone).await {
                            Ok(mut stream) => {
                                // Send observe commands separately
                                let observe_playlist = serde_json::json!({
                                    "command": ["observe_property", 1, "playlist-pos"]
                                });
                                let _ = stream.write_all(observe_playlist.to_string().as_bytes()).await;
                                let _ = stream.write_all(b"\n").await;

                                let observe_time = serde_json::json!({
                                    "command": ["observe_property", 2, "time-pos"]
                                });
                                let _ = stream.write_all(observe_time.to_string().as_bytes()).await;
                                let _ = stream.write_all(b"\n").await;

                                let mut buffer = String::new();
                                let mut reader = BufReader::new(stream);
                                while let Ok(bytes_read) = reader.read_line(&mut buffer).await {
                                    if bytes_read == 0 { break; }
                                    if let Ok(event) = serde_json::from_str::<Value>(buffer.trim()) {
                                        if let (Some(Value::String(name)), Some(n)) = (
                                            event.get("name"),
                                            event.get("data")
                                        ) {
                                            match name.as_str() {
                                                "playlist-pos" => {
                                                    if let Some(index) = n.as_i64().or_else(|| n.as_f64().map(|f| f as i64)) {
                                                        let new_index = index as usize;
                                                        println!("MPV event: playlist-pos → {}", new_index);
                                                        
                                                        // Handle -1 (no media playing) and out-of-bounds
                                                        if new_index < status_clone.songs.load(Ordering::Acquire) {
                                                            status_clone.current_index.store(new_index, Ordering::Release);
                                                            status_clone.force_ui_update.store(true, Ordering::Release);
                                                        }
                                                    }
                                                }
                                                "time-pos" => {
                                                    if let Some(time) = n.as_f64() {
                                                        status_clone.current_time.store((time * 1000.0) as u64, Ordering::Relaxed);
                                                    }
                                                }
                                                _ => {}
                                            }
                                        }
                                    }
                                    buffer.clear();
                                }
                            }
                            Err(_) => tokio::time::sleep(Duration::from_secs(1)).await,
                        }
                        
                        if status_clone.should_quit.load(Ordering::Acquire) {
                            break;
                        }
                        tokio::time::sleep(Duration::from_millis(100)).await;
                    }
                });
            }
            Err(e) => self.status_message = format!("Error: {}", e),
        }
        Ok(())
    }

    async fn stop_playback(&mut self) {
        self.player_status.should_quit.store(true, Ordering::Relaxed);
        if let Some(mut player) = self.current_player.take() {
            let _ = player.kill(); 
        }
        self.status_message = "Stopped".to_string();
        self.now_playing = None;

        self.player_status.current_index.store(usize::MAX, Ordering::Relaxed);
        self.player_status.should_quit.store(true, Ordering::Relaxed);
    }

    async fn update_now_playing(&mut self) {
        let current_index = self.player_status.current_index.load(Ordering::Acquire);
        let prev_index = self.now_playing.unwrap_or(usize::MAX);
        let songs_len = self.songs.len();
        
        // Handle index changes and wrap-around
        if current_index != prev_index {
            if current_index < songs_len {
                println!("Updating now playing from {} → {} (valid)", prev_index, current_index);
                self.now_playing = Some(current_index);
                self.song_state.selected = current_index;
                self.adjust_scroll();
                self.save_state()
                    .unwrap_or_else(|e| eprintln!("Failed to save state: {}", e));
            } else if current_index >= songs_len && songs_len > 0 {
                // Handle end of playlist
                println!("Playlist ended, resetting state");
                self.now_playing = None;
                self.player_status.current_index.store(usize::MAX, Ordering::Release);
                self.save_state()
                    .unwrap_or_else(|e| eprintln!("Failed to save state: {}", e));
            }
        }
    }

    fn get_now_playing_info(&self) -> String {
        self.now_playing
            .and_then(|i| self.songs.get(i))
            .map(|song| {
                let total_sec = song.duration;
                let total_min = total_sec / 60;
                let total_sec = total_sec % 60;
                
                let current_time_ms = self.player_status.current_time.load(Ordering::Relaxed);
                let current_time = current_time_ms as f64 / 1000.0;
                
                let current_min = current_time as u64 / 60;
                let current_sec = current_time as u64 % 60;
                
                let progress = (current_time / song.duration as f64 * 20.0) as usize;
                let progress_bar = format!("[{}{}]", 
                    "■".repeat(progress), 
                    " ".repeat(20 - progress)
                );

                format!(
                    "▶️ {:02}:{:02}/{:02}:{:02} {} - {}\n{}",
                    current_min, current_sec,
                    total_min, total_sec,
                    self.current_artist.as_ref().map(|a| a.name.as_str()).unwrap_or(""),
                    song.title,
                    progress_bar
                )
            })
            .unwrap_or_else(|| "⏹ No song playing".into())
    }
}

impl ViewMode { 
    fn previous(&self) -> Self {
        match self {
            ViewMode::Songs => ViewMode::Albums,
            ViewMode::Albums => ViewMode::Artists,
            _ => ViewMode::Artists,
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new().await?;
    let mut last_ui_update = Instant::now();
    let ui_refresh_rate = Duration::from_millis(100);

    loop {
        if last_ui_update.elapsed() > ui_refresh_rate {
            app.update_now_playing().await;
            terminal.draw(|f| ui(f, &app))?;
            last_ui_update = Instant::now();
        }

        if event::poll(Duration::from_millis(50))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    match key.code {
                        KeyCode::Char('q') => {
                            app.stop_playback().await;
                            break;
                        }
                        KeyCode::Up => app.on_up(),
                        KeyCode::Down => app.on_down(),
                        KeyCode::Left => app.mode = app.mode.previous(),
                        KeyCode::Right | KeyCode::Enter => {
                            let config = read_config()?;
                            match app.mode {
                                ViewMode::Artists => app.load_albums(&config).await?,
                                ViewMode::Albums => app.load_songs(&config).await?,
                                ViewMode::Songs => app.start_playback(&config).await?,
                            }
                        }
                        KeyCode::Char(' ') => app.stop_playback().await,
                        _ => {}
                    }
                }
            }
        } else {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen, DisableMouseCapture)?;
    terminal.show_cursor()?;
    Ok(())
}

fn ui(frame: &mut Frame, app: &App) {
    let main_layout = Layout::vertical([
        Constraint::Min(3),
        Constraint::Length(3),
    ]).split(frame.size());

    let panels = Layout::horizontal([
        Constraint::Ratio(1, 3),
        Constraint::Ratio(1, 3),
        Constraint::Ratio(1, 3),
    ]).split(main_layout[0]);

    render_artists_panel(frame, app, panels[0]);
    render_albums_panel(frame, app, panels[1]);
    render_songs_panel(frame, app, panels[2]);

    let status_bar = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(2),
    ]).split(main_layout[1]);

    let status_block = Paragraph::new(app.status_message.clone())
        .style(Style::default().fg(Color::Black).bg(Color::DarkGray))
        .block(Block::default().borders(Borders::TOP));
    frame.render_widget(status_block, status_bar[0]);

    let now_playing = Paragraph::new(app.get_now_playing_info())
        .style(Style::default().fg(Color::Yellow).bg(Color::DarkGray))
        .block(Block::default());
    frame.render_widget(now_playing, status_bar[1]);
}


fn render_artists_panel(frame: &mut Frame, app: &App, area: Rect) {
    let title = format!(" Artists ({}) ", app.artists.len());
    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(if app.current_artist.is_some() {
            Style::default().fg(Color::LightCyan)
        } else {
            Style::default().fg(Color::Gray)
        });

    let items: Vec<ListItem> = app.artists
        .iter()
        .skip(app.artist_state.scroll)
        .take(area.height as usize - 2)
        .enumerate()
        .map(|(i, artist)| {
            let is_selected = app.artist_state.selected == i + app.artist_state.scroll;
            let style = if is_selected {
                Style::default().fg(Color::Blue)
            } else {
                Style::default().fg(Color::Gray)
            };
            ListItem::new(artist.name.clone()).style(style)
        })
        .collect();

    frame.render_widget(List::new(items).block(block), area);
}

fn render_albums_panel(frame: &mut Frame, app: &App, area: Rect) {
    let title = format!(" Albums ({}) ", app.albums.len());
    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(if app.current_album.is_some() {
            Style::default().fg(Color::LightCyan)
        } else {
            Style::default().fg(Color::Gray)
        });

    let items: Vec<ListItem> = app.albums
        .iter()
        .skip(app.album_state.scroll)
        .take(area.height as usize - 2)
        .enumerate()
        .map(|(i, album)| {
            let is_selected = app.album_state.selected == i + app.album_state.scroll;
            let is_active = app.current_album.as_ref().map(|a| a.id.as_str()) == Some(album.id.as_str());
            
            let style = if is_active {
                Style::default()
                    .fg(Color::Blue)
                    .add_modifier(Modifier::BOLD)
            } else if is_selected {
                Style::default().fg(Color::Blue)
            } else {
                Style::default().fg(Color::Gray)
            };

            let text = format!("{} ({})", album.name, album.year.unwrap_or(0));
            ListItem::new(text).style(style)
        })
        .collect();

    frame.render_widget(List::new(items).block(block), area);
}

fn render_songs_panel(frame: &mut Frame, app: &App, area: Rect) {
    let title = match &app.current_album {
        Some(album) => format!(" {} ({}) ", album.name, app.songs.len()),
        None => " Songs ".to_string(),
    };

    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Gray));

    let items: Vec<ListItem> = app.songs
        .iter()
        .skip(app.song_state.scroll)
        .take(area.height as usize - 2)
        .enumerate()
        .map(|(i, song)| {
            let absolute_index = i + app.song_state.scroll;
            let is_selected = app.song_state.selected == absolute_index;
            let is_playing = app.now_playing == Some(absolute_index);

            let style = if is_playing {
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
            } else if is_selected {
                Style::default().fg(Color::Blue)
            } else {
                Style::default().fg(Color::Gray)
            };

            let minutes = song.duration / 60;
            let seconds = song.duration % 60;
            let text = format!("{:02}:{:02} - {}", minutes, seconds, song.title);
            ListItem::new(text).style(style)
        })
        .collect();

    frame.render_widget(List::new(items).block(block), area);
}

async fn get_artists(config: &Config) -> Result<Vec<Artist>> {
    let client = reqwest::Client::new();
    let response = client
        .get(format!("{}/rest/getArtists", config.server.url))
        .query(&[
            ("u", config.server.username.as_str()),
            ("p", config.server.password.as_str()),
            ("v", "1.16.1"),
            ("c", "TerminalDrome"),
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
            ("c", "TerminalDrome"),
            ("f", "json"),
            ("id", artist_id),
        ])
        .send()
        .await?;

    let body = response.text().await?;
    let parsed: SubsonicResponse = serde_json::from_str(&body)?;

    match parsed.response.content {
        ContentType::Albums { artist } => Ok(artist.album),
        _ => anyhow::bail!("Unexpected response format for artist albums"),
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
            ("c", "TerminalDrome"),
            ("f", "json"),
            ("id", album_id),
        ])
        .send()
        .await?;

    let body = response.text().await?;
    let parsed: SubsonicResponse = serde_json::from_str(&body)?;

    match parsed.response.content {
        ContentType::Songs { album } => Ok(album.song),
        _ => anyhow::bail!("Unexpected response format for album songs"),
    }
}

fn read_config() -> Result<Config> {
    let config = fs::read_to_string("config.toml")?;
    Ok(toml::from_str(&config)?)
}