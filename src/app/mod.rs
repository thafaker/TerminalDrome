use std::{
    fs,
    path::Path,
    process::{Child, Command},
    sync::Arc,
    sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use anyhow::Result;
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::UnixStream,
};

use crate::api::{build_stream_url, endpoints::*, models::*};
use crate::config::Config;
use crate::visual::Visualizer;

// ── ViewMode ─────────────────────────────────────────────────────────────────

#[derive(Debug, PartialEq, Clone, Copy, Serialize, Deserialize)]
pub enum ViewMode {
    Artists,
    Albums,
    Songs,
    Playlists,
    PlaylistSongs,
    Jukebox,
    Visualizer,
}

impl Default for ViewMode {
    fn default() -> Self { ViewMode::Artists }
}

impl ViewMode {
    pub fn previous(&self) -> Self {
        match self {
            ViewMode::Songs         => ViewMode::Albums,
            ViewMode::Albums        => ViewMode::Artists,
            ViewMode::Artists       => ViewMode::Artists,
            ViewMode::PlaylistSongs => ViewMode::Playlists,
            ViewMode::Playlists     => ViewMode::Playlists,
            ViewMode::Jukebox       => ViewMode::Jukebox,
            ViewMode::Visualizer    => ViewMode::Visualizer,
        }
    }
}

// ── PanelState ────────────────────────────────────────────────────────────────

#[derive(Debug, Default, Serialize, Deserialize, Clone, Copy)]
pub struct PanelState {
    pub selected: usize,
    pub scroll:   usize,
}

// ── AppState (persistence) ────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize)]
pub struct AppState {
    pub mode:             ViewMode,
    pub artist_state:     PanelState,
    pub album_state:      PanelState,
    pub song_state:       PanelState,
    pub playlist_state:   PanelState,
    pub current_artist:   Option<Artist>,
    pub current_album:    Option<Album>,
    pub current_playlist: Option<Playlist>,
    pub now_playing:      Option<usize>,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            mode:             ViewMode::Artists,
            artist_state:     PanelState::default(),
            album_state:      PanelState::default(),
            song_state:       PanelState::default(),
            playlist_state:   PanelState::default(),
            current_artist:   None,
            current_album:    None,
            current_playlist: None,
            now_playing:      None,
        }
    }
}

// ── PlayerStatus ──────────────────────────────────────────────────────────────

#[derive(Default)]
pub struct PlayerStatus {
    pub current_index:            AtomicUsize,
    pub current_time:             AtomicU32,
    pub force_ui_update:          AtomicBool,
    pub should_quit:              AtomicBool,
    pub songs:                    AtomicUsize,
    pub current_scrobble_sent:    AtomicBool,
    pub current_now_playing_sent: AtomicBool,
}

// ── App ───────────────────────────────────────────────────────────────────────

pub struct App {
    pub artists:          Vec<Artist>,
    pub albums:           Vec<Album>,
    pub songs:            Vec<Song>,
    pub playlists:        Vec<Playlist>,
    pub mode:             ViewMode,
    pub prev_mode:        ViewMode,
    pub should_quit:      bool,
    pub current_player:   Option<Child>,
    pub status_message:   String,
    pub current_artist:   Option<Artist>,
    pub current_album:    Option<Album>,
    pub current_playlist: Option<Playlist>,
    pub artist_state:     PanelState,
    pub album_state:      PanelState,
    pub song_state:       PanelState,
    pub playlist_state:   PanelState,
    pub now_playing:      Option<usize>,
    pub temp_dir:         Option<tempfile::TempDir>,
    pub config:           Config,
    pub is_search_mode:   bool,
    pub search_query:     String,
    pub search_results:   Vec<Song>,
    pub player_status:    Arc<PlayerStatus>,
    pub search_history:   Vec<String>,
    pub is_help_mode:     bool,
    pub volume:           u16,
    pub is_muted:         bool,
    pub is_jukebox_mode:        bool,
    pub jukebox_trim_offset:    usize,
    pub jukebox_fetching:       bool,
    pub is_shuffle:             bool,
    pub visualizer:             Visualizer,
}

impl Drop for App {
    fn drop(&mut self) {
        if let Some(mut player) = self.current_player.take() {
            let _ = player.kill();
        }
        if let Some(ref temp_dir) = self.temp_dir {
            let _ = fs::remove_dir_all(temp_dir.path());
        }
    }
}

impl App {
    pub async fn new() -> Result<Self> {
        let config    = crate::config::read_config()?;
        let artists   = get_artists(&config).await?;
        let playlists = get_playlists(&config).await.unwrap_or_default();
        let loaded    = Self::load_state().unwrap_or_default();

        Ok(Self {
            config,
            artists,
            albums:           Vec::new(),
            songs:            Vec::new(),
            playlists,
            mode:             loaded.mode,
            prev_mode:        loaded.mode,
            should_quit:      false,
            current_player:   None,
            status_message:   String::new(),
            current_artist:   loaded.current_artist,
            current_album:    loaded.current_album,
            current_playlist: loaded.current_playlist,
            artist_state:     loaded.artist_state,
            album_state:      loaded.album_state,
            song_state:       loaded.song_state,
            playlist_state:   loaded.playlist_state,
            now_playing:      loaded.now_playing,
            volume:           50,
            is_muted:         false,
            is_help_mode:     false,
            is_search_mode:   false,
            search_query:     String::new(),
            search_results:   Vec::new(),
            search_history:   Vec::new(),
            player_status:    Arc::new(PlayerStatus {
                current_index:            AtomicUsize::new(usize::MAX),
                current_time:             AtomicU32::new(0),
                force_ui_update:          AtomicBool::new(false),
                should_quit:              AtomicBool::new(false),
                songs:                    AtomicUsize::new(0),
                current_scrobble_sent:    AtomicBool::new(false),
                current_now_playing_sent: AtomicBool::new(false),
            }),
            temp_dir:            None,
            is_jukebox_mode:     false,
            jukebox_trim_offset: 0,
            jukebox_fetching:    false,
            is_shuffle:          false,
            visualizer:          Visualizer::new(8),
        })
    }

    pub async fn reset_to_artist_view(&mut self) -> Result<()> {
        self.mode = ViewMode::Artists;
        self.albums.clear();
        self.songs.clear();
        self.current_album   = None;
        self.is_jukebox_mode = false;
        self.is_shuffle      = false;
        Ok(())
    }

    // ── Persistence ──────────────────────────────────────────────────────────

    fn state_file_path() -> std::path::PathBuf {
        ProjectDirs::from("com", "TerminalDrome", "TerminalDrome")
            .map(|d| {
                let dir = d.data_local_dir().to_path_buf();
                let _ = fs::create_dir_all(&dir);
                dir.join("state.json")
            })
            .unwrap_or_else(|| Path::new("state.json").to_path_buf())
    }

    pub fn save_state(&self) -> Result<()> {
        let state = AppState {
            mode:             self.mode,
            artist_state:     self.artist_state,
            album_state:      self.album_state,
            song_state:       self.song_state,
            playlist_state:   self.playlist_state,
            current_artist:   self.current_artist.clone(),
            current_album:    self.current_album.clone(),
            current_playlist: self.current_playlist.clone(),
            now_playing:      self.now_playing,
        };
        fs::write(Self::state_file_path(), serde_json::to_string(&state)?)?;
        Ok(())
    }

    pub fn load_state() -> Result<AppState> {
        let path = Self::state_file_path();
        if path.exists() {
            Ok(serde_json::from_str(&fs::read_to_string(path)?)?)
        } else {
            Ok(AppState::default())
        }
    }

    // ── Navigation ───────────────────────────────────────────────────────────

    pub fn current_state_mut(&mut self) -> &mut PanelState {
        match self.mode {
            ViewMode::Artists       => &mut self.artist_state,
            ViewMode::Albums        => &mut self.album_state,
            ViewMode::Songs         => &mut self.song_state,
            ViewMode::Playlists     => &mut self.playlist_state,
            ViewMode::PlaylistSongs => &mut self.song_state,
            ViewMode::Jukebox       => &mut self.song_state,
            ViewMode::Visualizer    => &mut self.song_state,
        }
    }

    pub fn on_down(&mut self) {
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
            ViewMode::Songs | ViewMode::PlaylistSongs | ViewMode::Jukebox | ViewMode::Visualizer => {
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

    pub fn on_up(&mut self) {
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
            ViewMode::Songs | ViewMode::PlaylistSongs | ViewMode::Jukebox | ViewMode::Visualizer => {
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

    pub fn adjust_scroll(&mut self) {
        let visible = 15usize;
        let state   = self.current_state_mut();
        if state.selected < state.scroll {
            state.scroll = state.selected;
        } else if state.selected >= state.scroll + visible {
            state.scroll = state.selected - visible + 1;
        }
    }

    pub fn adjust_album_scroll(&mut self) {
        let visible = 5usize;
        if self.album_state.selected < self.album_state.scroll {
            self.album_state.scroll = self.album_state.selected;
        } else if self.album_state.selected >= self.album_state.scroll + visible {
            self.album_state.scroll = self.album_state.selected - visible + 1;
        }
    }

    pub fn adjust_playlist_scroll(&mut self) {
        let visible = 15usize;
        if self.playlist_state.selected < self.playlist_state.scroll {
            self.playlist_state.scroll = self.playlist_state.selected;
        } else if self.playlist_state.selected >= self.playlist_state.scroll + visible {
            self.playlist_state.scroll = self.playlist_state.selected - visible + 1;
        }
    }

    // ── Data loading ──────────────────────────────────────────────────────────

    pub async fn load_albums(&mut self) -> Result<()> {
        self.albums.clear();
        self.current_album = None;
        self.songs.clear();
        self.now_playing   = None;
        self.album_state   = PanelState::default();
        if let Some(artist) = self.artists.get(self.artist_state.selected) {
            self.albums         = get_artist_albums(&artist.id, &self.config).await?;
            self.current_artist = Some(artist.clone());
            self.mode           = ViewMode::Albums;
        }
        Ok(())
    }

    pub async fn load_songs(&mut self) -> Result<()> {
        self.songs.clear();
        self.now_playing = None;
        self.is_shuffle  = false;
        if let Some(album) = self.albums.get(self.album_state.selected) {
            self.songs         = get_album_songs(&album.id, &self.config).await?;
            self.current_album = Some(album.clone());
            self.mode          = ViewMode::Songs;
            self.song_state.selected = 0;
            self.adjust_scroll();
            self.start_playback().await?;
        }
        Ok(())
    }

    pub async fn load_playlist_songs(&mut self) -> Result<()> {
        self.songs.clear();
        self.now_playing = None;
        self.is_shuffle  = false;
        if let Some(playlist) = self.playlists.get(self.playlist_state.selected) {
            self.songs            = get_playlist_songs(&playlist.id, &self.config).await?;
            self.current_playlist = Some(playlist.clone());
            self.mode             = ViewMode::PlaylistSongs;
            self.song_state.selected = 0;
            self.adjust_scroll();
            self.start_playback().await?;
        }
        Ok(())
    }

    // ── Shuffle ───────────────────────────────────────────────────────────────

    pub async fn shuffle_and_restart(&mut self) -> Result<()> {
        if self.songs.is_empty() { return Ok(()); }
        use rand::seq::SliceRandom;
        self.songs.shuffle(&mut rand::thread_rng());
        self.song_state.selected = 0;
        self.song_state.scroll   = 0;
        self.now_playing         = None;
        self.is_shuffle          = true;
        self.status_message      = "🔀 Shuffled!".to_string();
        self.start_playback().await
    }

    // ── Jukebox ───────────────────────────────────────────────────────────────

    pub async fn start_jukebox(&mut self) -> Result<()> {
        if let Some(mut player) = self.current_player.take() { let _ = player.kill(); }
        self.is_jukebox_mode     = true;
        self.jukebox_trim_offset = 0;
        self.jukebox_fetching    = false;
        self.is_shuffle          = false;
        self.current_artist      = None;
        self.current_album       = None;
        self.current_playlist    = None;
        self.albums.clear();
        self.album_state = PanelState::default();
        self.status_message = "🎉 Jukebox – Lade Songs…".to_string();
        let initial = get_random_songs(&self.config, 50).await?;
        if initial.is_empty() {
            self.status_message = "Jukebox: Keine Songs gefunden!".to_string();
            return Ok(());
        }
        self.songs          = initial;
        self.song_state     = PanelState::default();
        self.mode           = ViewMode::Jukebox;
        self.status_message = "🎉 Jukebox / Party Mode – Shuffle your library!".to_string();
        self.start_playback().await
    }

    pub async fn jukebox_tick(&mut self) -> Result<()> {
        if !self.is_jukebox_mode { return Ok(()); }
        let current = self.player_status.current_index.load(Ordering::Acquire);
        if current == usize::MAX { return Ok(()); }
        let total = self.songs.len();
        if !self.jukebox_fetching && total.saturating_sub(current) < 10 {
            self.jukebox_fetching = true;
            let config        = self.config.clone();
            let socket_path   = self.temp_dir
                .as_ref()
                .map(|t| t.path().join("mpv.sock").to_str().unwrap_or("").to_string())
                .unwrap_or_default();
            let new_songs = get_random_songs(&config, 30).await.unwrap_or_default();
            for song in &new_songs {
                let url = build_stream_url(&song.id, &config);
                let cmd = format!("loadfile {} append\n", url);
                if !socket_path.is_empty() {
                    if let Ok(mut stream) = UnixStream::connect(&socket_path).await {
                        let _ = stream.write_all(cmd.as_bytes()).await;
                    }
                }
            }
            self.songs.extend(new_songs);
            let trim_until = current.saturating_sub(5);
            if trim_until > 0 && self.songs.len() > 100 {
                self.songs.drain(..trim_until);
                self.jukebox_trim_offset += trim_until;
                let corrected = current.saturating_sub(trim_until);
                self.player_status.current_index.store(corrected, Ordering::Release);
                self.song_state.selected = self.song_state.selected.saturating_sub(trim_until);
                self.adjust_scroll();
            }
            self.player_status.songs.store(self.songs.len(), Ordering::Release);
            self.jukebox_fetching = false;
        }
        Ok(())
    }

    // ── Playback ──────────────────────────────────────────────────────────────

    pub async fn adjust_volume(&mut self, delta: i32) {
        self.volume = (self.volume as i32 + delta).clamp(0, 100) as u16;
        let cmd = format!("set volume {}\n", self.volume);
        self.send_mpv_command(&cmd).await;
    }

    pub async fn toggle_mute(&mut self) {
        self.is_muted = !self.is_muted;
        let cmd = format!("set mute {}\n", if self.is_muted { "yes" } else { "no" });
        self.send_mpv_command(&cmd).await;
        self.player_status.force_ui_update.store(true, Ordering::Relaxed);
    }

    pub async fn next_track(&mut self) { self.send_mpv_command("playlist-next\n").await; }
    pub async fn previous_track(&mut self) { self.send_mpv_command("playlist-prev\n").await; }

    pub async fn send_mpv_command(&self, cmd: &str) {
        if let Some(ref temp_dir) = self.temp_dir {
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

    pub async fn start_playback(&mut self) -> Result<()> {
        if let Some(mut player) = self.current_player.take() { let _ = player.kill(); }

        let start_index = self.song_state.selected.clamp(0, self.songs.len().saturating_sub(1));
        self.player_status.songs.store(self.songs.len(), Ordering::Release);
        self.player_status.current_index.store(usize::MAX, Ordering::Release);
        self.temp_dir = Some(tempfile::tempdir_in("/tmp")?);
        let socket_path     = self.temp_dir.as_ref().unwrap().path().join("mpv.sock");
        let socket_path_str = socket_path.to_str().unwrap().to_string();
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

        for song in &self.songs {
            command.arg(build_stream_url(&song.id, &self.config));
        }

        match command.spawn() {
            Ok(child) => {
                self.current_player = Some(child);
                let label = if self.is_jukebox_mode {
                    "🎉 Jukebox / Party Mode".to_string()
                } else if self.is_shuffle {
                    match self.mode {
                        ViewMode::PlaylistSongs =>
                            format!("🔀 {}", self.current_playlist.as_ref().map(|p| p.name.as_str()).unwrap_or("")),
                        _ =>
                            format!("🔀 {}", self.current_album.as_ref().map(|a| a.name.as_str()).unwrap_or("")),
                    }
                } else {
                    match self.mode {
                        ViewMode::PlaylistSongs =>
                            self.current_playlist.as_ref().map(|p| p.name.as_str()).unwrap_or("").to_string(),
                        _ =>
                            self.current_album.as_ref().map(|a| a.name.as_str()).unwrap_or("").to_string(),
                    }
                };
                self.status_message = format!("Playing: {}", label);

                let status_clone      = self.player_status.clone();
                let socket_path_clone = socket_path_str.clone();

                tokio::spawn(async move {
                    loop {
                        match UnixStream::connect(&socket_path_clone).await {
                            Ok(mut stream) => {
                                let obs_pos  = serde_json::json!({"command": ["observe_property", 1, "playlist-pos"]});
                                let obs_time = serde_json::json!({"command": ["observe_property", 2, "time-pos"]});
                                let _ = stream.write_all(obs_pos.to_string().as_bytes()).await;
                                let _ = stream.write_all(b"\n").await;
                                let _ = stream.write_all(obs_time.to_string().as_bytes()).await;
                                let _ = stream.write_all(b"\n").await;

                                let mut buf    = String::new();
                                let mut reader = BufReader::new(stream);
                                while let Ok(n) = reader.read_line(&mut buf).await {
                                    if n == 0 { break; }
                                    if let Ok(ev) = serde_json::from_str::<Value>(buf.trim()) {
                                        if let (Some(Value::String(name)), Some(data)) =
                                            (ev.get("name"), ev.get("data"))
                                        {
                                            match name.as_str() {
                                                "playlist-pos" => {
                                                    if let Some(idx) = data.as_i64().or_else(|| data.as_f64().map(|f| f as i64)) {
                                                        let i = idx as usize;
                                                        if i < status_clone.songs.load(Ordering::Acquire) {
                                                            status_clone.current_index.store(i, Ordering::Release);
                                                            status_clone.force_ui_update.store(true, Ordering::Release);
                                                        }
                                                    }
                                                }
                                                "time-pos" => {
                                                    if let Some(t) = data.as_f64() {
                                                        status_clone.current_time.store((t * 1000.0) as u32, Ordering::Relaxed);
                                                    }
                                                }
                                                _ => {}
                                            }
                                        }
                                    }
                                    buf.clear();
                                }
                            }
                            Err(_) => tokio::time::sleep(Duration::from_secs(1)).await,
                        }
                        if status_clone.should_quit.load(Ordering::Acquire) { break; }
                        tokio::time::sleep(Duration::from_millis(100)).await;
                    }
                });
            }
            Err(e) => self.status_message = format!("Error starting mpv: {}", e),
        }
        Ok(())
    }

    pub async fn stop_playback(&mut self) {
        self.player_status.should_quit.store(true, Ordering::Relaxed);
        if let Some(mut player) = self.current_player.take() { let _ = player.kill(); }
        self.visualizer.stop_ffmpeg_feeder();
        self.status_message      = "Stopped".to_string();
        self.now_playing         = None;
        self.is_jukebox_mode     = false;
        self.jukebox_trim_offset = 0;
        self.is_shuffle          = false;
        self.player_status.current_index.store(usize::MAX, Ordering::Relaxed);
        self.player_status.should_quit.store(false, Ordering::Relaxed);
        self.player_status.force_ui_update.store(true, Ordering::Relaxed);
    }

    pub async fn update_now_playing(&mut self) {
        let current_index = self.player_status.current_index.load(Ordering::Acquire);
        let prev_index    = self.now_playing.unwrap_or(usize::MAX);
        let songs_len     = self.songs.len();

        if current_index != prev_index {
            if current_index < songs_len {
                self.player_status.current_scrobble_sent.store(false, Ordering::Release);
                self.player_status.current_now_playing_sent.store(false, Ordering::Release);
                self.now_playing         = Some(current_index);
                self.song_state.selected = current_index;
                self.adjust_scroll();
                self.save_state().unwrap_or_else(|e| eprintln!("Failed to save state: {}", e));
                // Restart ffmpeg feeder for new track if visualizer is active.
                // Always seek to 0 on track change — current_time still holds the
                // previous song's position and would cause ffmpeg to seek past EOF.
                if self.mode == ViewMode::Visualizer {
                    if let Some(fifo) = self.visualizer.fifo_path().map(|p| p.to_path_buf()) {
                        if let Some(song) = self.songs.get(current_index) {
                            let url = build_stream_url(&song.id, &self.config);
                            self.visualizer.start_ffmpeg_feeder(&url, &fifo, 0);
                        }
                    }
                }
            } else if songs_len > 0 && !self.is_jukebox_mode {
                self.now_playing = None;
                self.player_status.current_index.store(usize::MAX, Ordering::Release);
                self.save_state().unwrap_or_else(|e| eprintln!("Failed to save state: {}", e));
            }
        }
    }

    // ── Scrobbling ────────────────────────────────────────────────────────────

    pub async fn check_and_scrobble(&self) {
        let current_index = self.player_status.current_index.load(Ordering::Acquire);
        if current_index == usize::MAX { return; }
        let Some(song) = self.songs.get(current_index) else { return };

        let current_time_sec   = (self.player_status.current_time.load(Ordering::Relaxed) / 1000) as u64;
        let scrobble_threshold = std::cmp::min(10, song.duration / 2);

        if current_time_sec >= scrobble_threshold
            && !self.player_status.current_scrobble_sent.load(Ordering::Acquire)
        {
            let timestamp_ms = SystemTime::now()
                .duration_since(UNIX_EPOCH).unwrap().as_millis();
            if scrobble(&song.id, timestamp_ms, &self.config).await.is_ok() {
                self.player_status.current_scrobble_sent.store(true, Ordering::Release);
            }
        }
    }
}

pub fn normalize_for_search(s: &str) -> String {
    s.to_ascii_lowercase()
        .replace("ä", "a").replace("ö", "o").replace("ü", "u").replace("ß", "ss")
}
