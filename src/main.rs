#[macro_use]
extern crate lazy_static;

use std::{
    collections::HashMap,
    io::{self, Cursor},
    sync::Mutex,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use image::{
    imageops::colorops::grayscale,
    io::Reader as ImageReader,
    imageops::FilterType,
};

use directories::ProjectDirs;
use std::sync::atomic::{AtomicUsize, AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;
use std::{
    error::Error,
    fs,
    path::Path,
    process::{Child, Command},
};

use anyhow::Result;
use crossterm::{
    event::{
        self,
        Event,
        KeyCode,
        KeyEventKind,
        KeyModifiers,
        DisableMouseCapture,
        EnableMouseCapture,
    },
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Layout, Rect},
    prelude::{Alignment, Frame, Line, Span},
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, List, ListItem, Paragraph},
    Terminal,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tempfile;
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::UnixStream,
};
use rand::Rng;

// ============================================================
// CONFIG
// ============================================================

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

// ============================================================
// AUTH
// ============================================================

/// Erzeugt Token-basierte Auth-Parameter nach Subsonic API >= 1.13.0.
/// token = md5(password + salt), salt = zufällige alphanumerische Zeichenkette.
/// Das Passwort wird damit NIEMALS im Klartext übertragen.
struct AuthParams {
    user: String,
    token: String,
    salt: String,
}

impl AuthParams {
    fn new(config: &Config) -> Self {
        let salt: String = rand::thread_rng()
            .sample_iter(&rand::distributions::Alphanumeric)
            .take(8)
            .map(char::from)
            .collect();

        let token = format!("{:x}", md5::compute(format!("{}{}", config.server.password, salt)));

        Self {
            user: config.server.username.clone(),
            token,
            salt,
        }
    }
}

/// Gibt einen Vec von Query-Parametern zurück, der direkt an `.query()` übergeben werden kann.
/// Enthält u, t, s, v, c. Das Passwort taucht NICHT auf.
fn build_auth_query(config: &Config) -> Vec<(String, String)> {
    let auth = AuthParams::new(config);
    vec![
        ("u".to_string(),   auth.user),
        ("t".to_string(),   auth.token),
        ("s".to_string(),   auth.salt),
        ("v".to_string(),   "1.16.1".to_string()),
        ("c".to_string(),   "TerminalDrome".to_string()),
        ("f".to_string(),   "json".to_string()),
    ]
}

/// Baut eine Stream-URL für mpv. Da mpv die URL direkt öffnet (kein reqwest),
/// müssen wir hier Token + Salt in die URL einbetten.
/// Das Klartext-Passwort erscheint damit NICHT mehr in Prozesslisten oder Logs.
fn build_stream_url(song_id: &str, config: &Config) -> String {
    let auth = AuthParams::new(config);
    format!(
        "{}/rest/stream?id={}&u={}&t={}&s={}&v=1.16.1&c=TerminalDrome&f=json",
        config.server.url,
        song_id,
        auth.user,
        auth.token,
        auth.salt,
    )
}

// ============================================================
// SUBSONIC API DATA STRUCTURES
// ============================================================

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
    Artists         { artists: ArtistList },
    Albums          { artist: ArtistDetail },
    Songs           { album: AlbumDetail },
    Directory       (MusicDirectory),
    SearchResults   { searchResult3: SearchResult },
    Playlists       { playlists: PlaylistList },
    PlaylistDetail  { playlist: PlaylistSongs },
}

// --- Artists ---

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

// --- Albums ---

#[derive(Debug, Deserialize)]
struct ArtistDetail {
    album: Vec<Album>,
}

#[derive(Debug, Deserialize, Clone, Serialize)]
struct Album {
    id: String,
    name: String,
    artist: String,
    #[serde(rename = "coverArt")]
    cover_art: Option<String>,
    year: Option<i32>,
    #[serde(rename = "songCount")]
    song_count: u32,
}

#[derive(Debug, Deserialize)]
struct AlbumDetail {
    song: Vec<Song>,
}

// --- Songs ---

#[derive(Debug, Deserialize, Clone, Serialize)]
struct Song {
    id: String,
    title: String,
    duration: u64,
    track: Option<u32>,
    artist: Option<String>,
    album: Option<String>,
}

#[derive(Debug, Deserialize)]
struct MusicDirectory {
    child: Vec<Song>,
}

#[derive(Debug, Deserialize)]
struct SearchResult {
    song: Vec<Song>,
}

// --- Playlists ---

#[derive(Debug, Deserialize, Clone, Serialize)]
struct Playlist {
    id: String,
    name: String,
    #[serde(rename = "songCount")]
    song_count: u32,
    #[serde(default)]
    duration: u64,
    #[serde(rename = "coverArt")]
    cover_art: Option<String>,
    comment: Option<String>,
}

#[derive(Debug, Deserialize)]
struct PlaylistList {
    playlist: Vec<Playlist>,
}

#[derive(Debug, Deserialize)]
struct PlaylistSongs {
    // Navidrome liefert "entry" für Songs innerhalb einer Playlist
    #[serde(default)]
    entry: Vec<Song>,
}

// ============================================================
// APP STATE / VIEW MODES
// ============================================================

#[derive(Debug, PartialEq, Clone, Copy, Serialize, Deserialize)]
enum ViewMode {
    Artists,
    Albums,
    Songs,
    Playlists,
    PlaylistSongs,
}

impl ViewMode {
    fn previous(&self) -> Self {
        match self {
            ViewMode::Songs         => ViewMode::Albums,
            ViewMode::Albums        => ViewMode::Artists,
            ViewMode::Artists       => ViewMode::Artists,
            ViewMode::PlaylistSongs => ViewMode::Playlists,
            ViewMode::Playlists     => ViewMode::Playlists,
        }
    }
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
    playlist_state: PanelState,
    current_artist: Option<Artist>,
    current_album: Option<Album>,
    current_playlist: Option<Playlist>,
    now_playing: Option<usize>,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            mode: ViewMode::Artists,
            artist_state: PanelState::default(),
            album_state: PanelState::default(),
            song_state: PanelState::default(),
            playlist_state: PanelState::default(),
            current_artist: None,
            current_album: None,
            current_playlist: None,
            now_playing: None,
        }
    }
}

// ============================================================
// PLAYER STATUS (shared atomic state für den mpv-Monitor-Task)
// ============================================================

#[derive(Default)]
struct PlayerStatus {
    current_index:              AtomicUsize,
    /// Millisekunden (AtomicU32 reicht für ~49 Tage; in der Praxis kein Problem)
    current_time:               AtomicU32,
    force_ui_update:            AtomicBool,
    should_quit:                AtomicBool,
    songs:                      AtomicUsize,
    current_scrobble_sent:      AtomicBool,
    current_now_playing_sent:   AtomicBool,
}

// ============================================================
// COVER ART CACHE
// ============================================================

lazy_static! {
    static ref COVER_CACHE: Mutex<HashMap<String, String>> = Mutex::new(HashMap::new());
}

// ============================================================
// APP STRUCT
// ============================================================

struct App {
    artists:        Vec<Artist>,
    albums:         Vec<Album>,
    songs:          Vec<Song>,
    playlists:      Vec<Playlist>,
    mode:           ViewMode,
    should_quit:    bool,
    current_player: Option<Child>,
    status_message: String,
    current_artist: Option<Artist>,
    current_album:  Option<Album>,
    current_playlist: Option<Playlist>,
    artist_state:   PanelState,
    album_state:    PanelState,
    song_state:     PanelState,
    playlist_state: PanelState,
    now_playing:    Option<usize>,
    temp_dir:       Option<tempfile::TempDir>,
    config:         Config,
    is_search_mode: bool,
    search_query:   String,
    search_results: Vec<Song>,
    player_status:  Arc<PlayerStatus>,
    search_history: Vec<String>,
    is_help_mode:   bool,
    volume:         u16,
    is_muted:       bool,
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

// ============================================================
// APP - KONSTRUKTOR & INITIALISIERUNG
// ============================================================

impl App {
    async fn new() -> Result<Self> {
        let config  = read_config()?;
        let artists = get_artists(&config).await?;
        let playlists = get_playlists(&config).await.unwrap_or_default();

        let loaded_state = Self::load_state().unwrap_or_default();

        Ok(Self {
            config,
            artists,
            albums:         Vec::new(),
            songs:          Vec::new(),
            playlists,
            mode:           loaded_state.mode,
            should_quit:    false,
            current_player: None,
            status_message: String::new(),
            current_artist: loaded_state.current_artist.clone(),
            current_album:  loaded_state.current_album.clone(),
            current_playlist: loaded_state.current_playlist.clone(),
            artist_state:   loaded_state.artist_state,
            album_state:    loaded_state.album_state,
            song_state:     loaded_state.song_state,
            playlist_state: loaded_state.playlist_state,
            now_playing:    loaded_state.now_playing,
            volume:         50,
            is_muted:       false,
            is_help_mode:   false,
            is_search_mode: false,
            search_query:   String::new(),
            search_results: Vec::new(),
            search_history: Vec::new(),
            player_status:  Arc::new(PlayerStatus {
                current_index:            AtomicUsize::new(usize::MAX),
                current_time:             AtomicU32::new(0),
                force_ui_update:          AtomicBool::new(false),
                should_quit:              AtomicBool::new(false),
                songs:                    AtomicUsize::new(0),
                current_scrobble_sent:    AtomicBool::new(false),
                current_now_playing_sent: AtomicBool::new(false),
            }),
            temp_dir: None,
        })
    }

    async fn reset_to_artist_view(&mut self) -> Result<()> {
        self.mode = ViewMode::Artists;
        self.albums.clear();
        self.songs.clear();
        self.current_album = None;
        Ok(())
    }
}

// ============================================================
// APP - STATE PERSISTENZ
// ============================================================

impl App {
    fn state_file_path() -> std::path::PathBuf {
        ProjectDirs::from("com", "TerminalDrome", "TerminalDrome")
            .map(|d| {
                let dir = d.data_local_dir().to_path_buf();
                let _ = fs::create_dir_all(&dir);
                dir.join("state.json")
            })
            .unwrap_or_else(|| Path::new("state.json").to_path_buf())
    }

    fn save_state(&self) -> Result<()> {
        let state = AppState {
            mode: self.mode,
            artist_state: self.artist_state,
            album_state: self.album_state,
            song_state: self.song_state,
            playlist_state: self.playlist_state,
            current_artist: self.current_artist.clone(),
            current_album: self.current_album.clone(),
            current_playlist: self.current_playlist.clone(),
            now_playing: self.now_playing,
        };
        let state_json = serde_json::to_string(&state)?;
        fs::write(Self::state_file_path(), state_json)?;
        Ok(())
    }

    fn load_state() -> Result<AppState> {
        let path = Self::state_file_path();
        if path.exists() {
            let state_json = fs::read_to_string(path)?;
            Ok(serde_json::from_str(&state_json)?)
        } else {
            Ok(AppState::default())
        }
    }
}

// ============================================================
// APP - NAVIGATION / SCROLLING
// ============================================================

impl App {
    fn current_state_mut(&mut self) -> &mut PanelState {
        match self.mode {
            ViewMode::Artists       => &mut self.artist_state,
            ViewMode::Albums        => &mut self.album_state,
            ViewMode::Songs         => &mut self.song_state,
            ViewMode::Playlists     => &mut self.playlist_state,
            ViewMode::PlaylistSongs => &mut self.song_state,
        }
    }

    fn on_down(&mut self) {
        match self.mode {
            ViewMode::Artists => {
                let max = self.artists.len().saturating_sub(1);
                if self.artist_state.selected < max {
                    self.artist_state.selected += 1;
                    self.adjust_scroll();
                }
            }
            ViewMode::Albums => {
                let max = self.albums.len().saturating_sub(1);
                if self.album_state.selected < max {
                    self.album_state.selected += 1;
                    self.adjust_album_scroll();
                }
            }
            ViewMode::Songs | ViewMode::PlaylistSongs => {
                let max = self.songs.len().saturating_sub(1);
                if self.song_state.selected < max {
                    self.song_state.selected += 1;
                    self.adjust_scroll();
                }
            }
            ViewMode::Playlists => {
                let max = self.playlists.len().saturating_sub(1);
                if self.playlist_state.selected < max {
                    self.playlist_state.selected += 1;
                    self.adjust_playlist_scroll();
                }
            }
        }
    }

    fn on_up(&mut self) {
        match self.mode {
            ViewMode::Artists => {
                if self.artist_state.selected > 0 {
                    self.artist_state.selected -= 1;
                    self.adjust_scroll();
                }
            }
            ViewMode::Albums => {
                if self.album_state.selected > 0 {
                    self.album_state.selected -= 1;
                    self.adjust_album_scroll();
                }
            }
            ViewMode::Songs | ViewMode::PlaylistSongs => {
                if self.song_state.selected > 0 {
                    self.song_state.selected -= 1;
                    self.adjust_scroll();
                }
            }
            ViewMode::Playlists => {
                if self.playlist_state.selected > 0 {
                    self.playlist_state.selected -= 1;
                    self.adjust_playlist_scroll();
                }
            }
        }
    }

    fn adjust_scroll(&mut self) {
        let state = self.current_state_mut();
        let visible = 15usize;
        if state.selected < state.scroll {
            state.scroll = state.selected;
        } else if state.selected >= state.scroll + visible {
            state.scroll = state.selected - visible + 1;
        }
    }

    fn adjust_album_scroll(&mut self) {
        let visible = 5usize;
        if self.album_state.selected < self.album_state.scroll {
            self.album_state.scroll = self.album_state.selected;
        } else if self.album_state.selected >= self.album_state.scroll + visible {
            self.album_state.scroll = self.album_state.selected - visible + 1;
        }
    }

    fn adjust_playlist_scroll(&mut self) {
        let visible = 15usize;
        if self.playlist_state.selected < self.playlist_state.scroll {
            self.playlist_state.scroll = self.playlist_state.selected;
        } else if self.playlist_state.selected >= self.playlist_state.scroll + visible {
            self.playlist_state.scroll = self.playlist_state.selected - visible + 1;
        }
    }
}

// ============================================================
// APP - DATEN LADEN
// ============================================================

impl App {
    async fn load_albums(&mut self) -> Result<()> {
        self.albums.clear();
        self.current_album = None;
        self.songs.clear();
        self.now_playing = None;
        self.album_state = PanelState::default(); // Reset: kein stale Index in neuer Albumliste

        if let Some(artist) = self.artists.get(self.artist_state.selected) {
            self.albums = get_artist_albums(&artist.id, &self.config).await?;
            self.current_artist = Some(artist.clone());
            self.mode = ViewMode::Albums;
        }
        Ok(())
    }

    async fn load_songs(&mut self) -> Result<()> {
        self.songs.clear();
        self.now_playing = None;

        if let Some(album) = self.albums.get(self.album_state.selected) {
            self.songs = get_album_songs(&album.id, &self.config).await?;
            self.current_album = Some(album.clone());
            self.mode = ViewMode::Songs;
            self.song_state.selected = 0;
            self.adjust_scroll();
            self.start_playback().await?;
        }
        Ok(())
    }

    async fn load_playlist_songs(&mut self) -> Result<()> {
        self.songs.clear();
        self.now_playing = None;

        if let Some(playlist) = self.playlists.get(self.playlist_state.selected) {
            self.songs = get_playlist_songs(&playlist.id, &self.config).await?;
            self.current_playlist = Some(playlist.clone());
            self.mode = ViewMode::PlaylistSongs;
            self.song_state.selected = 0;
            self.adjust_scroll();
            self.start_playback().await?;
        }
        Ok(())
    }

    async fn search_songs(query: &str, config: &Config) -> Result<Vec<Song>> {
        let client = reqwest::Client::new();
        let mut params = build_auth_query(config);
        params.push(("query".to_string(),     query.to_string()));
        params.push(("songCount".to_string(), "100".to_string()));

        let response = client
            .get(format!("{}/rest/search3", config.server.url))
            .query(&params)
            .send()
            .await?;

        let body = response.text().await?;
        let parsed: SubsonicResponse = match serde_json::from_str(&body) {
            Ok(p) => p,
            Err(e) => {
                eprintln!("JSON Parse Error: {}", e);
                anyhow::bail!("Failed to parse search response");
            }
        };

        match parsed.response.content {
            ContentType::SearchResults { searchResult3 } => Ok(searchResult3.song),
            other => {
                eprintln!("Unexpected search response format: {:#?}", other);
                Ok(Vec::new())
            }
        }
    }
}

// ============================================================
// APP - WIEDERGABE / MPV
// ============================================================

impl App {
    async fn adjust_volume(&mut self, delta: i32) {
        self.volume = (self.volume as i32 + delta).clamp(0, 100) as u16;
        let cmd = format!("set volume {}\n", self.volume);
        self.send_mpv_command(&cmd).await;
    }

    async fn toggle_mute(&mut self) {
        self.is_muted = !self.is_muted;
        let cmd = format!("set mute {}\n", if self.is_muted { "yes" } else { "no" });
        self.send_mpv_command(&cmd).await;
        self.player_status.force_ui_update.store(true, Ordering::Relaxed);
    }

    async fn next_track(&mut self) {
        self.send_mpv_command("playlist-next\n").await;
    }

    async fn previous_track(&mut self) {
        self.send_mpv_command("playlist-prev\n").await;
    }

    async fn send_mpv_command(&self, cmd: &str) {
        if let Some(temp_dir) = &self.temp_dir {
            let socket_path = temp_dir.path().join("mpv.sock");
            match UnixStream::connect(socket_path).await {
                Ok(mut stream) => {
                    if let Err(e) = stream.write_all(cmd.as_bytes()).await {
                        eprintln!("MPV command error: {}", e);
                    }
                }
                Err(e) => eprintln!("MPV connection error: {}", e),
            }
        }
    }

    async fn start_playback(&mut self) -> Result<()> {
        if let Some(mut player) = self.current_player.take() {
            let _ = player.kill();
        }

        let start_index = self.song_state.selected.clamp(0, self.songs.len().saturating_sub(1));
        self.player_status.songs.store(self.songs.len(), Ordering::Release);
        self.player_status.current_index.store(usize::MAX, Ordering::Release);
        self.temp_dir = Some(tempfile::tempdir_in("/tmp")?);
        let socket_path = self.temp_dir.as_ref().unwrap().path().join("mpv.sock");
        let socket_path_str = match socket_path.to_str() {
            Some(s) => s.to_string(),
            None => {
                // Sehr selten, aber falls der Temp-Pfad nicht UTF-8 ist, lieber sauber abbrechen
                // statt per unwrap zu paniken.
                anyhow::bail!("Temp socket path is not valid UTF-8");
            }
        };
        self.player_status.force_ui_update.store(true, Ordering::Release);
        self.now_playing = Some(start_index);

        let mut command = Command::new("mpv");
        command
            .arg("--no-video")
            .arg(format!("--volume={}", self.volume))
            .arg(format!("--playlist-start={}", start_index))
            .arg("--really-quiet")
            .arg("--no-terminal")
            .arg("--audio-display=no")
            .arg("--loop-playlist=no")
            .arg("--msg-level=all=error")
            .arg(format!("--input-ipc-server={}", socket_path_str));

        // Stream-URLs mit Token-Auth (kein Klartext-Passwort in der Prozessliste)
        for song in &self.songs {
            command.arg(build_stream_url(&song.id, &self.config));
        }

        match command.spawn() {
            Ok(child) => {
                self.current_player = Some(child);
                let label = match self.mode {
                    ViewMode::PlaylistSongs =>
                        self.current_playlist.as_ref().map(|p| p.name.as_str()).unwrap_or("").to_string(),
                    _ =>
                        self.current_album.as_ref().map(|a| a.name.as_str()).unwrap_or("").to_string(),
                };
                self.status_message = format!("Playing: {}", label);

                let status_clone     = self.player_status.clone();
                let socket_path_clone = socket_path_str.clone();

                tokio::spawn(async move {
                    loop {
                        match UnixStream::connect(&socket_path_clone).await {
                            Ok(mut stream) => {
                                let obs_pos = serde_json::json!({
                                    "command": ["observe_property", 1, "playlist-pos"]
                                });
                                let _ = stream.write_all(obs_pos.to_string().as_bytes()).await;
                                let _ = stream.write_all(b"\n").await;

                                let obs_time = serde_json::json!({
                                    "command": ["observe_property", 2, "time-pos"]
                                });
                                let _ = stream.write_all(obs_time.to_string().as_bytes()).await;
                                let _ = stream.write_all(b"\n").await;

                                let mut buffer = String::new();
                                let mut reader = BufReader::new(stream);
                                while let Ok(bytes_read) = reader.read_line(&mut buffer).await {
                                    if bytes_read == 0 { break; }
                                    if let Ok(event) = serde_json::from_str::<Value>(buffer.trim()) {
                                        if let (Some(Value::String(name)), Some(data)) = (
                                            event.get("name"),
                                            event.get("data"),
                                        ) {
                                            match name.as_str() {
                                                "playlist-pos" => {
                                                    if let Some(index) = data.as_i64().or_else(|| data.as_f64().map(|f| f as i64)) {
                                                        let new_index = index as usize;
                                                        if new_index < status_clone.songs.load(Ordering::Acquire) {
                                                            status_clone.current_index.store(new_index, Ordering::Release);
                                                            status_clone.force_ui_update.store(true, Ordering::Release);
                                                        }
                                                    }
                                                }
                                                "time-pos" => {
                                                    if let Some(time) = data.as_f64() {
                                                        status_clone.current_time.store((time * 1000.0) as u32, Ordering::Relaxed);
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
            Err(e) => self.status_message = format!("Error starting mpv: {}", e),
        }

        // mpv benoetigt manchmal einen Moment, bis es das IPC-Socket anlegt.
        // Ohne diese kurze Pause verpassen wir ggf. die ersten IPC-Events.
        tokio::time::sleep(Duration::from_millis(150)).await;

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
        self.player_status.should_quit.store(false, Ordering::Relaxed);
        self.player_status.force_ui_update.store(true, Ordering::Relaxed);
    }

    async fn update_now_playing(&mut self) {
        let current_index = self.player_status.current_index.load(Ordering::Acquire);
        let prev_index    = self.now_playing.unwrap_or(usize::MAX);
        let songs_len     = self.songs.len();

        if current_index != prev_index {
            if current_index < songs_len {
                self.player_status.current_scrobble_sent.store(false, Ordering::Release);
                self.player_status.current_now_playing_sent.store(false, Ordering::Release);
                self.now_playing = Some(current_index);
                self.song_state.selected = current_index;
                self.adjust_scroll();
                self.save_state().unwrap_or_else(|e| eprintln!("Failed to save state: {}", e));
            } else if songs_len > 0 {
                self.now_playing = None;
                self.player_status.current_index.store(usize::MAX, Ordering::Release);
                self.save_state().unwrap_or_else(|e| eprintln!("Failed to save state: {}", e));
            }
        }
    }

    fn get_now_playing_info(&self) -> String {
        self.now_playing
            .and_then(|i| self.songs.get(i))
            .map(|song| {
                let total_sec   = song.duration;
                let current_sec = (self.player_status.current_time.load(Ordering::Relaxed) / 1000) as u64;
                let progress    = ((current_sec as f64 / total_sec.max(1) as f64) * 30.0) as usize;
                let progress    = progress.min(30);
                let bar         = format!("{}{}", "█".repeat(progress), "░".repeat(30 - progress));
                format!(
                    "{}\n{:02}:{:02}/{:02}:{:02}\n{}",
                    song.title,
                    current_sec / 60, current_sec % 60,
                    total_sec   / 60, total_sec   % 60,
                    bar
                )
            })
            .unwrap_or_else(|| "[.] Stopped".into())
    }
}

// ============================================================
// APP - SCROBBLING
// ============================================================

impl App {
    async fn check_and_scrobble(&self) {
        let current_index = self.player_status.current_index.load(Ordering::Acquire);
        if current_index == usize::MAX { return; }
        let Some(song) = self.songs.get(current_index) else { return };

        let current_time_sec = (self.player_status.current_time.load(Ordering::Relaxed) / 1000) as u64;
        let scrobble_threshold = std::cmp::min(10, song.duration / 2);

        if current_time_sec >= scrobble_threshold
            && !self.player_status.current_scrobble_sent.load(Ordering::Acquire)
        {
            let client = reqwest::Client::new();
            let timestamp_ms = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_millis();

            let mut params = build_auth_query(&self.config);
            params.push(("id".to_string(),          song.id.clone()));
            params.push(("time".to_string(),         timestamp_ms.to_string()));
            params.push(("submission".to_string(),   "true".to_string()));

            let response = client
                .get(format!("{}/rest/scrobble", self.config.server.url))
                .query(&params)
                .send()
                .await;

            if let Ok(resp) = response {
                if resp.status().is_success() {
                    self.player_status.current_scrobble_sent.store(true, Ordering::Release);
                } else if let Ok(body) = resp.text().await {
                    eprintln!("Scrobble failed: {}", body);
                }
            }
        }
    }
}

// ============================================================
// COVER ART
// ============================================================

async fn get_ascii_cover(album: Option<&Album>, config: &Config) -> String {
    let Some(album) = album else { return default_cover_art(); };
    let Some(cover_id) = &album.cover_art else { return default_cover_art(); };

    {
        let cache = COVER_CACHE.lock().unwrap();
        if let Some(cached) = cache.get(cover_id) {
            return cached.clone();
        }
    }

    let cover_width = 30;
    match fetch_cover_art(cover_id, config).await {
        Ok(img_data) => {
            let ascii = image_to_ascii(&img_data, cover_width)
                .unwrap_or_else(|_| default_cover_art());
            COVER_CACHE.lock().unwrap().insert(cover_id.clone(), ascii.clone());
            ascii
        }
        Err(e) => {
            eprintln!("Error loading cover art: {}", e);
            default_cover_art()
        }
    }
}

async fn fetch_cover_art(cover_id: &str, config: &Config) -> Result<Vec<u8>> {
    let mut params = build_auth_query(config);
    params.push(("id".to_string(), cover_id.to_string()));

    let client = reqwest::Client::new();
    let response = client
        .get(format!("{}/rest/getCoverArt", config.server.url))
        .query(&params)
        .send()
        .await?;

    Ok(response.bytes().await?.to_vec())
}

fn image_to_ascii(img_data: &[u8], width: u32) -> Result<String> {
    let aspect_ratio = 2.2;
    let height = (width as f32 / aspect_ratio) as u32;

    let img = ImageReader::new(Cursor::new(img_data))
        .with_guessed_format()?
        .decode()?
        .resize_exact(width * 2, height, FilterType::Triangle);

    let grayscale = grayscale(&img);
    let chars = [" ", "░", "▒", "▓", "█", "@", "#", "S", "%", "?", "*", "+", ";", ":", ",", "."];

    let mut ascii = String::with_capacity((width * height) as usize);
    for y in 0..grayscale.height() {
        for x in 0..grayscale.width() {
            let pixel      = grayscale.get_pixel(x, y);
            let brightness = pixel[0] as f32 / 255.0;
            let adjusted   = brightness.powf(1.8);
            let index      = (adjusted * (chars.len() - 1) as f32).round() as usize;
            ascii.push_str(chars[index]);
        }
        ascii.push('\n');
    }
    Ok(ascii)
}

fn default_cover_art() -> String {
    r#"
   ___
  / __\_____   _____ _ __
 / /  / _ \ \ / / _ \ '__|
/ /__| (_) \ V /  __/ |
\____/\___/ \_/ \___|_|
  /\  /\___ _ __ ___
 / /_/ / _ \ '__/ _ \
/ __  /  __/ | |  __/
\/ /_/ \___|_|  \___|
    "#.trim().to_string()
}

// ============================================================
// SUBSONIC API FUNKTIONEN
// ============================================================

async fn get_artists(config: &Config) -> Result<Vec<Artist>> {
    let client   = reqwest::Client::new();
    let params   = build_auth_query(config);
    let response = client
        .get(format!("{}/rest/getArtists", config.server.url))
        .query(&params)
        .send()
        .await?;

    let body   = response.text().await?;
    let parsed: SubsonicResponse = serde_json::from_str(&body)?;

    match parsed.response.content {
        ContentType::Artists { artists } =>
            Ok(artists.index.into_iter().flat_map(|g| g.artist).collect()),
        _ => anyhow::bail!("Unexpected response format for artists"),
    }
}

async fn get_artist_albums(artist_id: &str, config: &Config) -> Result<Vec<Album>> {
    let client = reqwest::Client::new();
    let mut params = build_auth_query(config);
    params.push(("id".to_string(), artist_id.to_string()));

    let response = client
        .get(format!("{}/rest/getArtist", config.server.url))
        .query(&params)
        .send()
        .await?;

    let body   = response.text().await?;
    let parsed: SubsonicResponse = serde_json::from_str(&body)?;

    match parsed.response.content {
        ContentType::Albums { artist } => Ok(artist.album),
        _ => anyhow::bail!("Unexpected response format for artist albums"),
    }
}

async fn get_album_songs(album_id: &str, config: &Config) -> Result<Vec<Song>> {
    let client = reqwest::Client::new();
    let mut params = build_auth_query(config);
    params.push(("id".to_string(), album_id.to_string()));

    let response = client
        .get(format!("{}/rest/getAlbum", config.server.url))
        .query(&params)
        .send()
        .await?;

    let body   = response.text().await?;
    let parsed: SubsonicResponse = serde_json::from_str(&body)?;

    match parsed.response.content {
        ContentType::Songs { album } => Ok(album.song),
        _ => anyhow::bail!("Unexpected response format for album songs"),
    }
}

async fn get_playlists(config: &Config) -> Result<Vec<Playlist>> {
    let client   = reqwest::Client::new();
    let params   = build_auth_query(config);
    let response = client
        .get(format!("{}/rest/getPlaylists", config.server.url))
        .query(&params)
        .send()
        .await?;

    let body = response.text().await?;
    let parsed: SubsonicResponse = serde_json::from_str(&body)?;

    match parsed.response.content {
        ContentType::Playlists { playlists } => Ok(playlists.playlist),
        _ => anyhow::bail!("Unexpected response format for playlists"),
    }
}

async fn get_playlist_songs(playlist_id: &str, config: &Config) -> Result<Vec<Song>> {
    let client = reqwest::Client::new();
    let mut params = build_auth_query(config);
    params.push(("id".to_string(), playlist_id.to_string()));

    let response = client
        .get(format!("{}/rest/getPlaylist", config.server.url))
        .query(&params)
        .send()
        .await?;

    let body = response.text().await?;
    let parsed: SubsonicResponse = serde_json::from_str(&body)?;

    match parsed.response.content {
        ContentType::PlaylistDetail { playlist } => Ok(playlist.entry),
        _ => anyhow::bail!("Unexpected response format for playlist songs"),
    }
}

// ============================================================
// CONFIG
// ============================================================

fn read_config() -> Result<Config> {
    let config_name = "config.toml";

    // 1. Aktuelles Verzeichnis
    let local_path = Path::new(config_name);
    if local_path.exists() {
        return parse_config(local_path);
    }

    // 2. XDG/System-Konfigurationsverzeichnis
    if let Some(proj_dirs) = ProjectDirs::from("com", "TerminalDrome", "TerminalDrome") {
        let config_path = proj_dirs.config_dir().join(config_name);
        if config_path.exists() {
            return parse_config(&config_path);
        }
    }

    // 3. Fehler anzeigen und beenden
    let error_msg = format!(
        "Config file not found!\n\n\
        Required paths:\n\
        - {}\n\
        - ./config.toml\n\n\
        Config template:\n{}",
        ProjectDirs::from("com", "TerminalDrome", "TerminalDrome")
            .map(|d| d.config_dir().join("config.toml").display().to_string())
            .unwrap_or_else(|| "~/.config/TerminalDrome/config.toml".into()),
        generate_config_template()
    );

    show_error_message(&error_msg);
    std::process::exit(1);
}

fn parse_config(path: &Path) -> Result<Config> {
    let raw = fs::read_to_string(path)?;
    let mut config: Config = toml::from_str(&raw)?;

    if !config.server.url.starts_with("https://") {
        config.server.url = config.server.url.replacen("http://", "https://", 1);
    }

    Ok(config)
}

fn generate_config_template() -> String {
    r#"[server]
url      = "https://your-navidrome-server.com"
username = "your-username"
password = "your-password"
"#.to_string()
}

fn show_error_message(error: &str) {
    let mut stdout = io::stdout();
    let _ = execute!(stdout, EnterAlternateScreen);
    let _ = enable_raw_mode();

    let backend  = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend).unwrap();

    terminal.draw(|f| {
        let chunks = Layout::vertical([
            Constraint::Percentage(20),
            Constraint::Min(10),
            Constraint::Percentage(20),
        ]).split(f.size());

        let error_block = Paragraph::new(error)
            .block(
                Block::default()
                    .title(" Critical Error ")
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Red))
                    .style(Style::default().bg(Color::Black))
            )
            .style(Style::default().fg(Color::White))
            .alignment(Alignment::Left);

        f.render_widget(error_block, chunks[1]);
    }).unwrap();

    let _ = event::read().unwrap();
    let _ = disable_raw_mode();
    let _ = execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    );
}

// ============================================================
// HILFSFUNKTIONEN
// ============================================================

fn normalize_for_search(s: &str) -> String {
    s.to_ascii_lowercase()
        .replace("ä", "a")
        .replace("ö", "o")
        .replace("ü", "u")
        .replace("ß", "ss")
}

// ============================================================
// MAIN
// ============================================================

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    std::panic::set_hook(Box::new(|panic_info| {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen, DisableMouseCapture);
        eprintln!("Panic occurred: {:?}", panic_info);
    }));

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend  = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Splash-Screen
    // HINWEIS: Ich hasse alles und die Welt weil ich hier verzweifle.
    // Kotze mit Erdbeeren!
    let raw_lines = vec![
         r"                                                      ",
        r"  This is:                                            ",
        r"    _______                  _             _          ",
        r"   |__   __|                (_)           | |         ",
        r"      | | ___ _ __ _ __ ___  _ _ __   __ _| |        ",
        r"      | |/ _ \ '__| '_ ` _ \| | '_ \ / _` | |       ",
        r"      | |  __/ |  | | | | | | | | | | (_| | |        ",
        r"    __|_|\___|_|  |_| |_| |_|_|_| |_|\__,_|_|       ",
        r"   |  __ \                                            ",
        r"   | |  | |_ __ ___  _ __ ___   ___                  ",
        r"   | |  | | '__/ _ \| '_ ` _ \ / _ \                ",
        r"   | |__| | | | (_) | | | | | |  __/                 ",
        r"   |_____/|_|  \___/|_| |_| |_|\___|                 ",
        r"                                                     ",
        r"   v0.3.2                       by Jan Montag        ",
        r"   Coded with love       in Mitteldeutschland         ",
        r"                                                     ",
    ];
    let splash_width  = raw_lines.iter().map(|l| l.len()).max().unwrap_or(54) as u16;
    let splash_height = raw_lines.len() as u16;
    let splash_text   = raw_lines.join("\n");

    terminal.draw(|f| {
        let sz = f.size();
        // Widget als Ganzes zentrieren - NICHT per Alignment::Center (wuerde jede Zeile einzeln verschieben)
        let x    = sz.width.saturating_sub(splash_width) / 2;
        let y    = sz.height.saturating_sub(splash_height) / 2;
        let area = Rect {
            x,
            y,
            width:  splash_width.min(sz.width),
            height: splash_height.min(sz.height),
        };
        let splash = Paragraph::new(splash_text.as_str())
            .style(Style::default().fg(Color::LightBlue))
            .alignment(Alignment::Left);
        f.render_widget(splash, area);
    })?;

    tokio::time::sleep(Duration::from_secs(2)).await;

    let mut app = App::new().await?;
    app.reset_to_artist_view().await?;

    let mut last_ui_update = Instant::now();
    let ui_refresh_rate    = Duration::from_millis(100);

    loop {
        if last_ui_update.elapsed() > ui_refresh_rate {
            app.update_now_playing().await;
            app.check_and_scrobble().await;
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
                            // Help
                            KeyCode::Char('H') if key.modifiers.contains(KeyModifiers::SHIFT) => {
                                app.is_help_mode = true;
                            }
                            // Quit
                            KeyCode::Char('Q') if key.modifiers.contains(KeyModifiers::SHIFT) => {
                                app.stop_playback().await;
                                app.should_quit = true;
                            }
                            // Volume
                            KeyCode::Char('+') | KeyCode::Char('=') => app.adjust_volume(5).await,
                            KeyCode::Char('-') => app.adjust_volume(-5).await,
                            // Mute
                            KeyCode::Char('m') if !app.is_search_mode => {
                                app.toggle_mute().await;
                            }
                            // Track navigation
                            KeyCode::Char('n') if !app.is_search_mode => app.next_track().await,
                            KeyCode::Char('p') if !app.is_search_mode => app.previous_track().await,
                            // Playlist-Ansicht umschalten (Tab)
                            KeyCode::Tab if !app.is_search_mode => {
                                match app.mode {
                                    ViewMode::Playlists | ViewMode::PlaylistSongs => {
                                        app.mode = ViewMode::Artists;
                                    }
                                    _ => {
                                        app.mode = ViewMode::Playlists;
                                    }
                                }
                            }
                            // Schnellsprung A-Z (außer reservierten Tasten)
                            KeyCode::Char(c)
                                if c.is_alphabetic()
                                && !app.is_search_mode
                                && !matches!(c, 'n' | 'p' | 'm' | 'h' | 'q') =>
                            {
                                let sc = c.to_ascii_lowercase().to_string();
                                match app.mode {
                                    ViewMode::Artists => {
                                        if let Some(pos) = app.artists.iter().position(|a| {
                                            normalize_for_search(&a.name).starts_with(&sc)
                                        }) {
                                            app.artist_state.selected = pos;
                                            app.adjust_scroll();
                                        }
                                    }
                                    ViewMode::Albums => {
                                        if let Some(pos) = app.albums.iter().position(|a| {
                                            normalize_for_search(&a.name).starts_with(&sc)
                                        }) {
                                            app.album_state.selected = pos;
                                            app.adjust_scroll();
                                        }
                                    }
                                    ViewMode::Songs | ViewMode::PlaylistSongs => {
                                        if let Some(pos) = app.songs.iter().position(|s| {
                                            normalize_for_search(&s.title).starts_with(&sc)
                                        }) {
                                            app.song_state.selected = pos;
                                            app.adjust_scroll();
                                        }
                                    }
                                    ViewMode::Playlists => {
                                        if let Some(pos) = app.playlists.iter().position(|pl| {
                                            normalize_for_search(&pl.name).starts_with(&sc)
                                        }) {
                                            app.playlist_state.selected = pos;
                                            app.adjust_playlist_scroll();
                                        }
                                    }
                                }
                            }
                            // Suche starten
                            KeyCode::Char('/') => {
                                app.is_search_mode = true;
                                app.search_query.clear();
                            }
                            KeyCode::Esc => app.is_search_mode = false,
                            // Suche ausführen
                            KeyCode::Enter if app.is_search_mode => {
                                let results = App::search_songs(&app.search_query, &app.config).await?;
                                app.search_results = results;
                                app.songs = app.search_results.clone();
                                app.search_history.push(app.search_query.clone());
                                app.current_artist  = None;
                                app.current_album   = None;
                                app.artist_state    = PanelState::default();
                                app.album_state     = PanelState::default();
                                app.song_state      = PanelState::default();
                                app.mode            = ViewMode::Songs;
                                app.is_search_mode  = false;
                                app.adjust_scroll();
                            }
                            KeyCode::Char(c) if app.is_search_mode => {
                                app.search_query.push(c);
                            }
                            KeyCode::Backspace if app.is_search_mode => {
                                app.search_query.pop();
                            }
                            // Navigation
                            KeyCode::Up   => app.on_up(),
                            KeyCode::Down => app.on_down(),
                            KeyCode::Left => {
                                app.mode = app.mode.previous();
                            }
                            KeyCode::Right | KeyCode::Enter => match app.mode {
                                ViewMode::Artists       => app.load_albums().await?,
                                ViewMode::Albums        => app.load_songs().await?,
                                ViewMode::Songs         => app.start_playback().await?,
                                ViewMode::Playlists     => app.load_playlist_songs().await?,
                                ViewMode::PlaylistSongs => app.start_playback().await?,
                            },
                            // Stop
                            KeyCode::Char(' ') => {
                                app.stop_playback().await;
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
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    Ok(())
}

// ============================================================
// UI
// ============================================================
// I hate RUST btw

fn ui(frame: &mut Frame, app: &App) {
    if app.is_help_mode {
        render_help(frame);
    } else if app.is_search_mode {
        render_search_input(frame, app);
    } else {
        render_main(frame, app);
    }
}

fn render_help(frame: &mut Frame) {
    let help_text = vec![
        Line::from(" TerminalDrome - Keyboard Shortcuts ").style(Style::default().fg(Color::Yellow)),
        Line::from(""),
        Line::from("> Navigation:"),
        Line::from("  Up/Dn    - Move selection"),
        Line::from("  Lt/Rt    - Switch views"),
        Line::from("  Enter  - Confirm selection"),
        Line::from("  Tab    - Toggle Playlists / Artists"),
        Line::from(""),
        Line::from("> Playback:"),
        Line::from("  Space  - Stop"),
        Line::from("  n      - Next track"),
        Line::from("  p      - Previous track"),
        Line::from("  +      - Volume up"),
        Line::from("  -      - Volume down"),
        Line::from("  m      - Toggle mute"),
        Line::from(""),
        Line::from("> Other:"),
        Line::from("  /      - Search"),
        Line::from("  A-Z    - Quick jump in lists"),
        Line::from("  Shift+Q - Quit"),
        Line::from("  Shift+H - This help screen"),
    ];

    let area = Rect {
        x:      frame.size().width / 4,
        y:      1,
        width:  frame.size().width / 2,
        height: 24,
    };

    frame.render_widget(
        Paragraph::new(help_text)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" Help ")
                    .border_style(Style::default().fg(Color::LightBlue))
            )
            .alignment(Alignment::Left),
        area,
    );
}

fn render_search_input(frame: &mut Frame, app: &App) {
    let area = Rect {
        x:      frame.size().width / 4,
        y:      frame.size().height / 2,
        width:  frame.size().width / 2,
        height: 3,
    };
    frame.render_widget(
        Paragraph::new(app.search_query.as_str())
            .style(Style::default().fg(Color::Yellow))
            .block(Block::default().borders(Borders::ALL).title(" Search ")),
        area,
    );
}

fn render_main(frame: &mut Frame, app: &App) {
    let main_layout = Layout::vertical([
        Constraint::Min(3),     // Panels
        Constraint::Length(1),  // Trennlinie 1
        Constraint::Length(1),  // Statuszeile
        Constraint::Length(1),  // Trennlinie 2
        Constraint::Length(1),  // Song-Info
        Constraint::Length(1),  // Fortschrittsbalken
    ]).split(frame.size());

    // Panels
    let panels = Layout::horizontal([
        Constraint::Ratio(2, 6),  // Artists / Playlists
        Constraint::Ratio(2, 6),  // Albums
        Constraint::Ratio(2, 6),  // Songs
    ]).split(main_layout[0]);

    // Linkes Panel: Playlists oder Artists
    match app.mode {
        ViewMode::Playlists | ViewMode::PlaylistSongs => {
            render_playlists_panel(frame, app, panels[0]);
        }
        _ => {
            render_artists_panel(frame, app, panels[0]);
        }
    }
    render_albums_panel(frame, app, panels[1]);
    render_songs_panel(frame, app, panels[2]);

    // Trennlinien
    let divider      = "-".repeat(frame.size().width as usize);
    let divider_style = Style::default().fg(Color::DarkGray);
    frame.render_widget(Paragraph::new(divider.clone()).style(divider_style), main_layout[1]);
    frame.render_widget(Paragraph::new(divider).style(divider_style),         main_layout[3]);

    // Statuszeile
    let mode_label = match app.mode {
        ViewMode::Playlists | ViewMode::PlaylistSongs => "Playlists",
        _ => "Artists",
    };
    // Statuszeile: kompakt genug fuer 80-Zeichen-Terminal.
    // Der Mode-Indikator (Artists/Playlists + Tab-Hint) sitzt im Panel-Titel des linken Panels -
    // dort ist er kontextuell sinnvoller und kostet hier keinen Platz.
    let mute_str = if app.is_muted { "ON" } else { "OFF" };
    let status_line = Paragraph::new(Line::from(vec![
        Span::styled(format!("VOL:{}% ", app.volume), Style::new().fg(Color::Cyan)),
        Span::raw("| "),
        Span::styled("MUTE:", Style::new().fg(Color::Magenta)),
        Span::styled(
            mute_str,
            Style::new().fg(if app.is_muted { Color::Red } else { Color::Green }),
        ),
        Span::raw(" | "),
        Span::styled("/", Style::new().fg(Color::Yellow)),
        Span::styled(":Search", Style::new().fg(Color::DarkGray)),
        Span::raw(" | "),
        Span::styled("Q", Style::new().fg(Color::Red).add_modifier(Modifier::BOLD)),
        Span::styled(":Quit", Style::new().fg(Color::DarkGray)),
        Span::raw(" | "),
        Span::styled("H", Style::new().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
        Span::styled(":Help", Style::new().fg(Color::DarkGray)),
        Span::raw(" | "),
        Span::styled("n/p", Style::new().fg(Color::Cyan)),
        Span::styled(":Tracks", Style::new().fg(Color::DarkGray)),
        Span::raw(" | "),
        Span::styled("Tab", Style::new().fg(Color::Magenta).add_modifier(Modifier::BOLD)),
        Span::styled(":Mode", Style::new().fg(Color::DarkGray)),
    ]));
    frame.render_widget(status_line, main_layout[2]);

    // Song-Info
    let song_info = app.now_playing
        .and_then(|i| app.songs.get(i))
        .map(|song| format!(
            "> {} - {}",
            song.artist.as_deref().unwrap_or("Unknown"),
            song.title
        ))
        .unwrap_or_else(|| "[.] Stopped".into());

    frame.render_widget(
        Paragraph::new(song_info).style(Style::default().fg(Color::Yellow)),
        main_layout[4],
    );

    // Fortschrittsbalken
    let (current, total) = app.now_playing
        .and_then(|i| app.songs.get(i))
        .map(|song| (
            (app.player_status.current_time.load(Ordering::Relaxed) as u64) / 1000,
            song.duration,
        ))
        .unwrap_or((0, 1));

    let bar_width = (frame.size().width as usize).saturating_sub(20).max(10);
    let progress  = if total > 0 { current as f32 / total as f32 } else { 0.0 };
    let filled    = (progress * bar_width as f32).round() as usize;

    let progress_bar = format!(
        "{:02}:{:02} |{}{}| {:02}:{:02}",
        current / 60, current % 60,
        "=".repeat(filled),
        "-".repeat(bar_width - filled),
        total / 60, total % 60,
    );

    frame.render_widget(
        Paragraph::new(progress_bar)
            .style(Style::default().fg(Color::Blue))
            .alignment(Alignment::Center),
        main_layout[5],
    );
}

// ============================================================
// PANEL RENDERER
// ============================================================

fn render_artists_panel(frame: &mut Frame, app: &App, area: Rect) {
    let title = if app.search_results.is_empty() {
        format!(" Artists ({}) [Tab<>] ", app.artists.len())
    } else {
        " Search Mode ".to_string()
    };

    // Rahmenfarbe: aktives Pane = White, playing-Kontext = LightCyan, inaktiv = DarkGray
    let is_active = matches!(app.mode, ViewMode::Artists);
    let border_color = if !app.search_results.is_empty() {
        Color::Yellow
    } else if is_active {
        Color::White
    } else if app.current_artist.is_some() {
        Color::LightCyan
    } else {
        Color::DarkGray
    };

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

    frame.render_widget(
        List::new(items).block(
            Block::default()
                .title(title)
                .borders(Borders::ALL)
                .border_style(Style::default().fg(border_color))
        ),
        area,
    );
}
// I hate RUST
fn render_playlists_panel(frame: &mut Frame, app: &App, area: Rect) {
    let title  = format!(" Playlists ({}) [Tab<>] ", app.playlists.len());
    let is_active = matches!(app.mode, ViewMode::Playlists | ViewMode::PlaylistSongs);
    let border = if is_active {
        Color::White
    } else if app.current_playlist.is_some() {
        Color::LightCyan
    } else {
        Color::DarkGray
    };

    let items: Vec<ListItem> = app.playlists
        .iter()
        .skip(app.playlist_state.scroll)
        .take(area.height as usize - 2)
        .enumerate()
        .map(|(i, pl)| {
            let abs        = i + app.playlist_state.scroll;
            let is_sel     = app.playlist_state.selected == abs;
            let is_active  = app.current_playlist.as_ref().map(|p| p.id.as_str()) == Some(pl.id.as_str());

            let style = if is_active {
                Style::default().fg(Color::Blue).add_modifier(Modifier::BOLD)
            } else if is_sel {
                Style::default().fg(Color::Blue)
            } else {
                Style::default().fg(Color::Gray)
            };

            let text = format!("# {} ({})", pl.name, pl.song_count);
            ListItem::new(text).style(style)
        })
        .collect();

    frame.render_widget(
        List::new(items).block(
            Block::default()
                .title(title)
                .borders(Borders::ALL)
                .border_style(Style::default().fg(border))
        ),
        area,
    );
}

fn render_albums_panel(frame: &mut Frame, app: &App, area: Rect) {
    let chunks = Layout::vertical([
        Constraint::Length(12),
        Constraint::Min(3),
    ]).split(area);

    let is_active_albums = matches!(app.mode, ViewMode::Albums);
    let border_color = if !app.search_results.is_empty() {
        Color::Yellow
    } else if is_active_albums {
        Color::White
    } else if app.current_album.is_some() {
        Color::LightCyan
    } else {
        Color::DarkGray
    };

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

    let lines: Vec<&str>  = current_cover.lines().collect();
    let total_lines        = lines.len().max(1);
    let colored_ascii: Vec<Line> = lines
        .into_iter()
        .enumerate()
        .map(|(y, line)| {
            let g     = y as f32 / total_lines as f32;
            let color = Color::Rgb(
                (255.0 * (1.0 - g)) as u8,
                (255.0 * g) as u8,
                128,
            );
            Line::from(Span::styled(line, Style::default().fg(color)))
        })
        .collect();

    frame.render_widget(
        Paragraph::new(colored_ascii)
            .block(
                Block::default()
                    .title(" Cover Art ")
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Magenta))
            )
            .alignment(Alignment::Center),
        chunks[0],
    );

    // Album-Liste
    let title = if app.search_results.is_empty() {
        match app.albums.len() {
            0 => " Albums ".to_string(),
            n => format!(" Albums ({}) ", n),
        }
    } else {
        " Results ".to_string()
    };

    let items: Vec<ListItem> = app.albums
        .iter()
        .skip(app.album_state.scroll)
        .take(chunks[1].height as usize - 2)
        .enumerate()
        .map(|(i, album)| {
            let abs      = i + app.album_state.scroll;
            let is_sel   = app.album_state.selected == abs;
            let is_active = app.current_album.as_ref().map(|a| a.id.as_str()) == Some(album.id.as_str());

            let style = if is_active {
                Style::default().fg(Color::Blue).add_modifier(Modifier::BOLD)
            } else if is_sel {
                Style::default().fg(Color::Blue)
            } else {
                Style::default().fg(Color::Gray)
            };

            ListItem::new(format!("{} ({})", album.name, album.year.unwrap_or(0))).style(style)
        })
        .collect();

    frame.render_widget(
        List::new(items).block(
            Block::default()
                .title(title)
                .borders(Borders::ALL)
                .border_style(Style::default().fg(border_color))
        ),
        chunks[1],
    );
}

fn render_songs_panel(frame: &mut Frame, app: &App, area: Rect) {
    let title = if !app.search_results.is_empty() {
        format!(" Search: {} ({}) ", app.search_query, app.songs.len())
    } else {
        match app.mode {
            ViewMode::PlaylistSongs =>
                app.current_playlist.as_ref()
                    .map(|p| format!(" # {} ({}) ", p.name, app.songs.len()))
                    .unwrap_or_else(|| " Playlist Songs ".to_string()),
            _ =>
                app.current_album.as_ref()
                    .map(|a| format!(" {} ({}) ", a.name, app.songs.len()))
                    .unwrap_or_else(|| " Songs ".to_string()),
        }
    };

    let is_active_songs = matches!(app.mode, ViewMode::Songs | ViewMode::PlaylistSongs);
    let border_style = if !app.search_results.is_empty() {
        Style::default().fg(Color::Yellow)
    } else if is_active_songs {
        Style::default().fg(Color::White)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let items: Vec<ListItem> = app.songs
        .iter()
        .skip(app.song_state.scroll)
        .take(area.height as usize - 2)
        .enumerate()
        .map(|(i, song)| {
            let abs        = i + app.song_state.scroll;
            let is_sel     = app.song_state.selected == abs;
            let is_playing = app.now_playing == Some(abs);

            let style = if is_playing {
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
            } else if is_sel {
                Style::default().fg(Color::Blue)
            } else {
                Style::default().fg(Color::Gray)
            };

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
        List::new(items).block(
            Block::default()
                .title(title)
                .borders(Borders::ALL)
                .border_style(border_style)
        ),
        area,
    );
}