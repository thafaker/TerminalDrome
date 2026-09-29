use std::collections::HashMap;
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

// ── SongInfoOverlay ───────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct SongInfoOverlay {
    pub fallback_song:    Song,
    pub detail:           Option<SongDetail>,
    pub error:            Option<String>,
    pub local_play_count: u32,
}

/// A full screen view of the lyrics of the currently playing song.
///
/// This is a separate overlay rather than another section of the song info
/// box, because that box is already full on an 80x25 terminal. A lyric that
/// cannot be seen is worse than one that is not offered.
#[derive(Debug)]
pub struct LyricsOverlay {
    pub title:  String,
    pub artist: Option<String>,
    pub state:  LyricsState,
    /// First visible line, in rows.
    pub scroll: u16,
    /// The song the loaded lyrics belong to. Playback may have moved on while
    /// the overlay was open, and the highlight has to notice that instead of
    /// walking through the previous song's words with the new song's clock.
    pub song_id: String,
    /// Playback position the view is showing.
    ///
    /// Held here rather than read from the player at draw time, because the
    /// freeze on a song change has to cover the highlight as well as the
    /// scroll. The render has no way to know whether the words on screen still
    /// belong to what is playing, so it can only show what it is told.
    pub now_ms:  u64,
    /// Whether the view is following playback. Scrolling by hand turns this
    /// off, because a page that scrolls itself back is unreadable.
    pub follow:  bool,
}

impl LyricsOverlay {
    /// Scroll by `delta` rows, which may be negative.
    ///
    /// Clamped against the rows actually on screen, so scrolling past either
    /// end does nothing instead of leaving a blank view. A hand scroll stops
    /// the view from following playback, which `Home` turns back on.
    pub fn scroll_by(&mut self, delta: i32, width: u16, visible_rows: u16) {
        let total = self.state.row_count_at(width);
        let max_scroll = total.saturating_sub(visible_rows);
        let next = (self.scroll as i32 + delta).clamp(0, max_scroll as i32);
        self.scroll = next as u16;
        self.follow = false;
    }

    /// Back to the top, and back to following playback from there.
    pub fn scroll_home(&mut self) {
        self.scroll = 0;
        self.follow = true;
    }

    /// To the end, used by the End key.
    pub fn scroll_end(&mut self, width: u16, visible_rows: u16) {
        self.scroll = self.state.row_count_at(width).saturating_sub(visible_rows);
        self.follow = false;
    }

    /// Move the highlight to wherever playback is and bring it into view.
    ///
    /// Does nothing unless the view is following, and freezes entirely when
    /// `same_song` is false: both the recorded clock and the scroll stop, so a
    /// song change cannot make the old words walk along with the new clock.
    /// Freezing rather than clearing is deliberate. The reader keeps the text
    /// they opened, and can still scroll it by hand, and the highlight simply
    /// stays where it was instead of jumping to a line that means nothing.
    pub fn follow_playback(
        &mut self,
        now_ms: u64,
        width: u16,
        visible_rows: u16,
        same_song: bool,
    ) {
        if !self.follow || !same_song { return; }
        self.now_ms = now_ms;

        let Some(active) = self.state.active_index(now_ms) else { return };

        let top = self.state.row_offset_at(width, active);
        // The last row of the line, so a lyric wrapping over several rows is
        // followed by its end and not by its first fragment.
        let bottom = self.state.row_offset_at(width, active + 1).saturating_sub(1);
        let bottom = std::cmp::max(bottom, top);
        let last_row = visible_rows.saturating_sub(1);

        // Scroll only once the line has left the window, so the page does not
        // creep upwards on every line that happens to be near an edge.
        if bottom < self.scroll || top > self.scroll + last_row {
            // Park the line about a third down rather than against an edge:
            // at the bottom there is nothing to read ahead into, and at the top
            // nothing that came before it.
            let anchor = visible_rows / 3;
            let wanted = top.saturating_sub(anchor);
            let max_scroll = self.state.row_count_at(width).saturating_sub(visible_rows);
            self.scroll = wanted.min(max_scroll);
        }
    }
}

/// Where the lyrics of the inspected song currently stand.
///
/// A track without lyrics is a normal outcome and gets its own state rather
/// than an error, so the UI can say "no lyrics for this track" without
/// suggesting that something went wrong.
#[derive(Debug, Clone, Default)]
pub enum LyricsState {
    #[default]
    NotAsked,
    Loading,
    /// The server answered. `is_empty()` on the payload means the track has
    /// none, which is not a failure.
    Ready(Box<Lyrics>),
    /// The request itself failed.
    Failed(String),
}

impl LyricsState {
    /// Columns the timestamp prefix takes in front of a timed lyric.
    ///
    /// The view formats it as `"  M:SS  "`, so this has to stay in step with
    /// that format or a wrapped line is measured one row short.
    pub const TIMESTAMP_COLS: usize = 10;

    /// Columns left for lyric text once the overlay's borders are taken off.
    fn inner_width(width: u16) -> u16 {
        width.saturating_sub(2).max(1)
    }

    /// Screen rows a rendered line of `cols` characters occupies.
    ///
    /// Ratatui wraps at word boundaries, which can need one row more than a
    /// straight division of the widths. Counting the short way is deliberate:
    /// being a row out on a very long line only shifts the follow position
    /// slightly, whereas a wrong count would clamp scrolling to the wrong end.
    fn rows_of(cols: usize, inner: u16) -> u16 {
        let len = cols as u16;
        if len == 0 { return 1; }
        std::cmp::max(len.div_ceil(inner), 1)
    }

    /// How many screen rows the lyrics need at the given terminal width.
    pub fn row_count_at(&self, width: u16) -> u16 {
        let inner = Self::inner_width(width);
        match self {
            LyricsState::Ready(l) if !l.is_empty() => {
                if l.lines.is_empty() {
                    l.value
                        .lines()
                        .filter(|t| !t.trim().is_empty())
                        .map(|t| Self::rows_of(t.trim().chars().count() + 2, inner))
                        .sum()
                } else {
                    l.lines
                        .iter()
                        .map(|x| Self::rows_of(x.value.chars().count() + Self::TIMESTAMP_COLS, inner))
                        .sum()
                }
            }
            _ => 1,
        }
    }

    /// Screen row at which the lyric with this source index starts.
    ///
    /// The view scrolls in rows, not in lyrics, so the follow position has to
    /// know how many rows the lines above occupied once they were wrapped.
    pub fn row_offset_at(&self, width: u16, line_index: usize) -> u16 {
        let inner = Self::inner_width(width);
        match self {
            LyricsState::Ready(l) if !l.is_empty() => {
                if l.lines.is_empty() {
                    l.value
                        .lines()
                        .filter(|t| !t.trim().is_empty())
                        .take(line_index)
                        .map(|t| Self::rows_of(t.trim().chars().count() + 2, inner))
                        .sum()
                } else {
                    l.lines
                        .iter()
                        .take(line_index)
                        .map(|x| Self::rows_of(x.value.chars().count() + Self::TIMESTAMP_COLS, inner))
                        .sum()
                }
            }
            _ => 0,
        }
    }

    /// Source index of the lyric the highlight belongs on right now, if the
    /// lyrics are timed at all and belong to the song that is playing.
    pub fn active_index(&self, now_ms: u64) -> Option<usize> {
        match self {
            LyricsState::Ready(l) => l.active_line(now_ms),
            _ => None,
        }
    }
}

// ── PlaylistPickerOverlay ─────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct PlaylistPickerOverlay {
    /// The song we want to add to a playlist.
    pub song:      Song,
    /// Current selection index in `app.playlists`.
    pub selected:  usize,
    /// Set while the "create new playlist" prompt is open.
    pub creating:  bool,
    /// Text buffer for the new playlist name.
    pub new_name:  String,
    /// Whether the currently highlighted playlist is being operated on.
    pub busy:      bool,
}

// ── AppState (persistence) ────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize)]
pub struct AppState {
    pub mode:             ViewMode,
    pub active_source:    MusicSource,
    pub artist_state:     PanelState,
    pub album_state:      PanelState,
    pub song_state:       PanelState,
    pub playlist_state:   PanelState,
    pub current_artist:   Option<Artist>,
    pub current_album:    Option<Album>,
    pub current_playlist: Option<Playlist>,
    pub now_playing:      Option<usize>,
    #[serde(default)]
    pub play_counts:      HashMap<String, u32>,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            mode:             ViewMode::Artists,
            active_source:    MusicSource::Navidrome,
            artist_state:     PanelState::default(),
            album_state:      PanelState::default(),
            song_state:       PanelState::default(),
            playlist_state:   PanelState::default(),
            current_artist:   None,
            current_album:    None,
            current_playlist: None,
            now_playing:      None,
            play_counts:      HashMap::new(),
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
    /// From mpv's `pause` property, so the UI can show and reason about it.
    pub is_paused:                AtomicBool,
}

// ── App ───────────────────────────────────────────────────────────────────────

pub struct App {
    pub artists:                Vec<Artist>,
    pub albums:                 Vec<Album>,
    pub songs:                  Vec<Song>,
    pub playlists:              Vec<Playlist>,
    pub mode:                   ViewMode,
    pub prev_mode:              ViewMode,
    pub active_source:          MusicSource,
    pub should_quit:            bool,
    pub current_player:         Option<Child>,
    pub status_message:         String,
    pub current_artist:         Option<Artist>,
    pub current_album:          Option<Album>,
    pub current_playlist:       Option<Playlist>,
    pub artist_state:           PanelState,
    pub album_state:            PanelState,
    pub song_state:             PanelState,
    pub playlist_state:         PanelState,
    pub now_playing:            Option<usize>,
    pub temp_dir:               Option<tempfile::TempDir>,
    pub config:                 Config,
    pub is_search_mode:         bool,
    pub search_query:           String,
    pub search_results:         Vec<Song>,
    pub player_status:          Arc<PlayerStatus>,
    pub search_history:         Vec<String>,
    pub is_help_mode:           bool,
    pub volume:                 u16,
    pub is_muted:               bool,
    pub is_jukebox_mode:        bool,
    pub jukebox_trim_offset:    usize,
    pub jukebox_fetching:       bool,
    pub is_shuffle:             bool,
    pub visualizer:             Visualizer,
    pub play_counts:            HashMap<String, u32>,
    pub song_info_overlay:      Option<SongInfoOverlay>,
    pub lyrics_overlay:         Option<LyricsOverlay>,
    pub playlist_picker:        Option<PlaylistPickerOverlay>,
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
        let config = crate::config::read_config()?;
        let loaded = Self::load_state().unwrap_or_default();

        // The restored source may no longer be usable — the `[bandcamp]`
        // section could have been removed, disabled or left with placeholders.
        // Falling back silently would show "🎸 BC" while every request still
        // went to Navidrome, so say so instead.
        let mut status_message = String::new();
        let mut active_source = loaded.active_source;
        if active_source == MusicSource::Bandcamp
            && !is_source_usable(MusicSource::Bandcamp, &config)
        {
            active_source = MusicSource::Navidrome;
            status_message =
                "⚠️ Saved Bandcamp source is no longer configured — switched back to Navidrome"
                    .to_string();
        }

        let artists = get_artists(active_source, &config).await?;

        // A failing playlist fetch must not abort startup, but it should be
        // visible rather than looking like an account without playlists.
        let playlists = match get_playlists(active_source, &config).await {
            Ok(playlists) => playlists,
            Err(e) => {
                if status_message.is_empty() {
                    status_message = format!("⚠️ Could not load playlists: {}", e);
                }
                Vec::new()
            }
        };

        Ok(Self {
            config,
            artists,
            albums:           Vec::new(),
            songs:            Vec::new(),
            playlists,
            mode:             loaded.mode,
            prev_mode:        loaded.mode,
            active_source,
            should_quit:      false,
            current_player:   None,
            status_message,
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
                is_paused:                AtomicBool::new(false),
            }),
            temp_dir:            None,
            is_jukebox_mode:     false,
            jukebox_trim_offset: 0,
            jukebox_fetching:    false,
            is_shuffle:          false,
            visualizer:          Visualizer::new(8),
            play_counts:         loaded.play_counts,
            song_info_overlay:   None,
            lyrics_overlay:      None,
            playlist_picker:     None,
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
            active_source:    self.active_source,
            artist_state:     self.artist_state,
            album_state:      self.album_state,
            song_state:       self.song_state,
            playlist_state:   self.playlist_state,
            current_artist:   self.current_artist.clone(),
            current_album:    self.current_album.clone(),
            current_playlist: self.current_playlist.clone(),
            now_playing:      self.now_playing,
            play_counts:      self.play_counts.clone(),
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
            self.albums         = get_artist_albums(self.active_source, &artist.id, &self.config).await?;
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
            self.songs         = get_album_songs(self.active_source, &album.id, &self.config).await?;
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
            self.songs            = get_playlist_songs(self.active_source, &playlist.id, &self.config).await?;
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
        self.status_message = "🎉 Jukebox – Loading songs…".to_string();
        let initial = get_random_songs(self.active_source, &self.config, 50).await?;
        if initial.is_empty() {
            self.status_message = "Jukebox: No songs found!".to_string();
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
            let source        = self.active_source;
            let socket_path   = self.temp_dir
                .as_ref()
                .map(|t| t.path().join("mpv.sock").to_str().unwrap_or("").to_string())
                .unwrap_or_default();
            let new_songs = get_random_songs(source, &config, 30).await.unwrap_or_default();
            for song in &new_songs {
                let url = build_stream_url(&song.id, source, &config);
                // JSON IPC rather than the text form, because the per-file
                // title has to travel in the options map.
                let cmd = format!(
                    "{}\n",
                    loadfile_command(&url, "append", media_title(song).as_deref())
                );
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

    /// Pause/Resume, analog zu mpvs eigener Space-Belegung.
    ///
    /// `cycle pause` schaltet um, ohne dass der aktuelle Zustand bekannt sein
    /// muss. `is_paused` pflegt der MonitorTask ueber `observe_property`.
    /// Bewusst kein `stop_playback`: die Wiedergabe und damit auch der
    /// aktuelle Playlist-Eintrag bleiben erhalten.
    pub async fn toggle_pause(&mut self) {
        if self.player_status.current_index.load(Ordering::Acquire) == usize::MAX {
            self.status_message = "❌ Nothing is playing".to_string();
            return;
        }
        self.send_mpv_command("cycle pause\n").await;
        self.player_status.force_ui_update.store(true, Ordering::Relaxed);
    }

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

    pub async fn like_current_song(&mut self) -> Result<()> {
        let current_index = self.player_status.current_index.load(Ordering::Acquire);
        if current_index == usize::MAX {
            self.status_message = "❌ No song currently playing".to_string();
            return Ok(());
        }

        if let Some(song) = self.songs.get_mut(current_index) {
            match crate::api::endpoints::star_song(self.active_source, &song.id, &self.config).await {
                Ok(_) => {
                    song.starred = Some("true".to_string());
                    self.status_message = format!("❤️ Liked: {}", song.title);
                }
                Err(e) => {
                    self.status_message = format!("❌ Failed to like song: {}", e);
                }
            }
        }
        Ok(())
    }

    // ── Song Info Overlay ─────────────────────────────────────────────────────

    pub async fn open_song_info(&mut self) -> Result<()> {
        let current = self.player_status.current_index.load(Ordering::Acquire);
        if current == usize::MAX {
            self.status_message = "❌ No song currently playing".to_string();
            return Ok(());
        }
        let Some(song) = self.songs.get(current).cloned() else { return Ok(()); };

        let key              = play_count_key(self.active_source, &song.id);
        let local_play_count = self.play_counts.get(&key).copied().unwrap_or(0);

        // Open the overlay immediately with fallback data
        self.song_info_overlay = Some(SongInfoOverlay {
            fallback_song: song.clone(),
            detail: None,
            error: None,
            local_play_count,
        });

        // Fetch detailed metadata with a timeout so the UI never hangs
        let source = self.active_source;
        let config = self.config.clone();
        let result = tokio::time::timeout(
            Duration::from_secs(5),
            get_song_info(source, &song.id, &config),
        ).await;

        if let Some(overlay) = self.song_info_overlay.as_mut() {
            match result {
                Ok(Ok(detail)) => overlay.detail = Some(detail),
                Ok(Err(e))      => overlay.error  = Some(e.to_string()),
                Err(_)          => overlay.error  = Some("Request timed out".to_string()),
            }
        }
        Ok(())
    }

    /// Open the lyrics view for the song that is playing.
    ///
    /// The overlay appears straight away in a loading state so the key feels
    /// like it did something, and the lyrics arrive into it afterwards. The
    /// lookup is by artist and title rather than by song id, because that is
    /// what the plain Subsonic endpoint accepts.
    pub async fn open_lyrics(&mut self) -> Result<()> {
        let current = self.player_status.current_index.load(Ordering::Acquire);
        if current == usize::MAX {
            self.status_message = "❌ No song currently playing".to_string();
            return Ok(());
        }
        let Some(song) = self.songs.get(current).cloned() else { return Ok(()) };

        let artist = song.artist.clone().unwrap_or_default();
        let title  = song.title.clone();

        self.lyrics_overlay = Some(LyricsOverlay {
            title:  title.clone(),
            artist: song.artist.clone(),
            state:  LyricsState::Loading,
            scroll: 0,
            song_id: song.id.clone(),
            now_ms:  0,
            follow: true,
        });

        let source = self.active_source;
        let config = self.config.clone();
        let result = tokio::time::timeout(
            Duration::from_secs(5),
            async {
                // A track with no artist cannot be matched by the API.
                if artist.trim().is_empty() || title.trim().is_empty() {
                    return Ok(Lyrics::default());
                }
                get_lyrics(source, &song.id, &artist, &title, &config).await
            },
        )
        .await;

        if let Some(overlay) = self.lyrics_overlay.as_mut() {
            overlay.state = match result {
                Ok(Ok(l)) => LyricsState::Ready(Box::new(l)),
                Ok(Err(e)) => LyricsState::Failed(e.to_string()),
                Err(_)     => LyricsState::Failed("Request timed out".into()),
            };
        }
        Ok(())
    }

    pub fn close_lyrics(&mut self) {
        self.lyrics_overlay = None;
    }

    /// Move the lyric highlight along with playback.
    ///
    /// The overlay is told whether the loaded lyrics still belong to the track
    /// that is playing, because the view can stay open across a song change.
    pub fn tick_lyrics(&mut self, width: u16, visible_rows: u16) {
        let Some(o) = self.lyrics_overlay.as_mut() else { return };
        let now_ms = self.player_status.current_time.load(Ordering::Relaxed) as u64;
        let current = self.player_status.current_index.load(Ordering::Acquire);
        let same_song = self.songs.get(current).map(|s| s.id == o.song_id).unwrap_or(false);
        o.follow_playback(now_ms, width, visible_rows, same_song);
    }

    /// Scroll the lyrics view. `delta` is in rows and may be negative.
    pub fn scroll_lyrics(&mut self, delta: i32, width: u16, visible_rows: u16) {
        if let Some(o) = self.lyrics_overlay.as_mut() {
            o.scroll_by(delta, width, visible_rows);
        }
    }

    /// Back to the top, and back to following playback.
    pub fn scroll_lyrics_home(&mut self) {
        if let Some(o) = self.lyrics_overlay.as_mut() { o.scroll_home(); }
    }

    /// To the end, used by the End key.
    pub fn scroll_lyrics_end(&mut self, width: u16, visible_rows: u16) {
        if let Some(o) = self.lyrics_overlay.as_mut() {
            o.scroll_end(width, visible_rows);
        }
    }

    pub fn close_song_info(&mut self) {
        self.song_info_overlay = None;
    }

    // ── Playlist management ───────────────────────────────────────────────────

    /// Open the "add to playlist" picker for the currently playing song.
    pub async fn open_playlist_picker(&mut self) -> Result<()> {
        let current = self.player_status.current_index.load(Ordering::Acquire);
        if current == usize::MAX {
            self.status_message = "❌ No song currently playing".to_string();
            return Ok(());
        }
        let Some(song) = self.songs.get(current).cloned() else { return Ok(()); };

        // Ensure we have a fresh playlist list, especially if the user just
        // switched sources or created playlists elsewhere.
        if let Ok(fresh) = get_playlists(self.active_source, &self.config).await {
            self.playlists = fresh;
        }

        self.playlist_picker = Some(PlaylistPickerOverlay {
            song,
            selected: 0,
            creating: false,
            new_name: String::new(),
            busy:     false,
        });
        Ok(())
    }

    pub fn close_playlist_picker(&mut self) {
        self.playlist_picker = None;
    }

    /// Add the currently picked song to the selected playlist.
    pub async fn confirm_playlist_pick(&mut self) -> Result<()> {
        let Some(picker) = self.playlist_picker.as_mut() else { return Ok(()); };
        picker.busy = true;

        let song  = picker.song.clone();
        let index = picker.selected;

        let Some(playlist) = self.playlists.get(index).cloned() else {
            self.status_message = "❌ No playlist selected".to_string();
            self.close_playlist_picker();
            return Ok(());
        };

        match add_song_to_playlist(self.active_source, &playlist.id, &song.id, &self.config).await {
            Ok(_) => {
                self.status_message = format!("✅ Added to '{}'", playlist.name);
                // Bump the local song count so the UI reflects the change
                // without a round-trip refetch.
                if let Some(pl) = self.playlists.iter_mut().find(|p| p.id == playlist.id) {
                    pl.song_count = pl.song_count.saturating_add(1);
                }
            }
            Err(e) => {
                self.status_message = format!("❌ Failed to add: {}", e);
            }
        }
        self.close_playlist_picker();
        Ok(())
    }

    /// Create a new playlist containing the currently picked song.
    pub async fn create_playlist_with_song(&mut self) -> Result<()> {
        let Some(picker) = self.playlist_picker.as_ref() else { return Ok(()); };
        let name = picker.new_name.trim().to_string();
        let song = picker.song.clone();

        if name.is_empty() {
            self.status_message = "❌ Playlist name cannot be empty".to_string();
            return Ok(());
        }

        match create_playlist(self.active_source, &name, Some(&song.id), &self.config).await {
            Ok(_) => {
                self.status_message = format!("✅ Created '{}'", name);
                // Refresh the playlists list so the new entry appears.
                if let Ok(fresh) = get_playlists(self.active_source, &self.config).await {
                    self.playlists = fresh;
                }
            }
            Err(e) => {
                self.status_message = format!("❌ Failed to create: {}", e);
            }
        }
        self.close_playlist_picker();
        Ok(())
    }

    /// Remove a song from the currently open playlist.
    ///
    /// The index passed to the server is the position within the *server-side*
    /// playlist, which is what `self.songs` reflects right after loading.
    ///
    /// After a successful removal we must fix up every index that refers to
    /// a position in `self.songs` — otherwise a currently playing track would
    /// suddenly point at a different song (or out of range entirely).
    pub async fn remove_from_current_playlist(&mut self) -> Result<()> {
        if self.mode != ViewMode::PlaylistSongs {
            self.status_message =
                "❌ Not in a playlist view".to_string();
            return Ok(());
        }
        let Some(playlist) = self.current_playlist.as_ref().cloned() else {
            self.status_message = "❌ No playlist open".to_string();
            return Ok(());
        };
        let index = self.song_state.selected;
        if index >= self.songs.len() {
            return Ok(());
        }
        let song_title = self.songs[index].title.clone();

        match remove_song_from_playlist(self.active_source, &playlist.id, index, &self.config).await {
            Ok(_) => {
                // ── Remove locally ────────────────────────────────────────
                self.songs.remove(index);

                // ── Fix up the "now playing" pointer ─────────────────────
                //
                // Three cases, in order of what the player might be doing:
                //
                // 1. The removed song was the one currently playing →
                //    mpv will keep playing it (its own playlist is unaware
                //    of the removal). Leave the index alone and let mpv
                //    finish; UI stays consistent with the audio.
                //
                // 2. The removed song was *before* the current one →
                //    every following song shifted down by one, so the
                //    current pointer must decrease by one too.
                //
                // 3. The removed song was *after* the current one →
                //    no adjustment needed for the current pointer.
                let current = self.player_status.current_index.load(Ordering::Acquire);
                if current != usize::MAX && current > index {
                    let corrected = current - 1;
                    self.player_status.current_index.store(corrected, Ordering::Release);
                    if self.now_playing == Some(current) {
                        self.now_playing = Some(corrected);
                    }
                }

                // ── Fix up the list selection ─────────────────────────────
                if self.song_state.selected >= self.songs.len() && !self.songs.is_empty() {
                    self.song_state.selected = self.songs.len() - 1;
                }

                // ── Update the playlist's local song count ────────────────
                if let Some(pl) = self.playlists.iter_mut().find(|p| p.id == playlist.id) {
                    pl.song_count = pl.song_count.saturating_sub(1);
                }

                self.status_message = format!("🗑 Removed '{}'", song_title);
                self.adjust_scroll();

                // If we just shifted the current pointer, make sure the UI
                // knows to redraw the "now playing" row correctly.
                self.player_status.force_ui_update.store(true, Ordering::Release);
            }
            Err(e) => {
                self.status_message = format!("❌ Failed to remove: {}", e);
            }
        }
        Ok(())
    }

    // ── Playback (continued) ──────────────────────────────────────────────────

    pub async fn start_playback(&mut self) -> Result<()> {
        if let Some(mut player) = self.current_player.take() { let _ = player.kill(); }

        if self.songs.is_empty() {
            // The songs arrive over IPC, so mpv would sit there forever on an
            // empty playlist — clean up after it.
            self.temp_dir = None;
            self.status_message = "Nothing to play".to_string();
            return Ok(());
        }

        let start_index = self.song_state.selected.clamp(0, self.songs.len() - 1);
        self.player_status.songs.store(self.songs.len(), Ordering::Release);
        self.player_status.current_index.store(usize::MAX, Ordering::Release);
        self.player_status.is_paused.store(false, Ordering::Relaxed);
        self.temp_dir = Some(tempfile::tempdir_in("/tmp")?);
        let socket_path     = self.temp_dir.as_ref().unwrap().path().join("mpv.sock");
        let socket_path_str = socket_path.to_str().unwrap().to_string();
        self.player_status.force_ui_update.store(true, Ordering::Release);
        self.now_playing = Some(start_index);

        // Snapshot the playlist for mpv. Songs are not passed on the command
        // line any more, because a command-line `force-media-title` is global
        // and would apply one title to the whole playlist. Each song is loaded
        // over IPC with its own title instead — see `loadfile_command`.
        let playlist: Vec<(String, Option<String>)> = self
            .songs
            .iter()
            .map(|song| {
                (
                    build_stream_url(&song.id, self.active_source, &self.config),
                    media_title(song),
                )
            })
            .collect();

        let mut command = Command::new("mpv");
        command
            .arg("--no-video")
            .arg(format!("--volume={}", self.volume))
            .arg("--really-quiet")
            .arg("--no-terminal")
            .arg("--audio-display=no")
            .arg("--loop-playlist=no")
            .arg("--msg-level=all=error")
            // Stay alive with an empty playlist; the songs are pushed below.
            .arg("--idle=yes")
            .arg(format!("--input-ipc-server={}", socket_path_str));

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
                let playlist_clone    = playlist;

                tokio::spawn(async move {
                    let mut playlist_loaded = false;
                    loop {
                        match UnixStream::connect(&socket_path_clone).await {
                            Ok(mut stream) => {
                                // Observe before loading, otherwise the event
                                // for the track we select would fire before
                                // anything is watching and `current_index`
                                // would stay unset.
                                let obs_pos  = serde_json::json!({"command": ["observe_property", 1, "playlist-pos"]});
                                let obs_time = serde_json::json!({"command": ["observe_property", 2, "time-pos"]});
                                let obs_paus = serde_json::json!({"command": ["observe_property", 3, "pause"]});
                                let _ = stream.write_all(obs_pos.to_string().as_bytes()).await;
                                let _ = stream.write_all(b"\n").await;
                                let _ = stream.write_all(obs_time.to_string().as_bytes()).await;
                                let _ = stream.write_all(b"\n").await;
                                let _ = stream.write_all(obs_paus.to_string().as_bytes()).await;
                                let _ = stream.write_all(b"\n").await;

                                if !playlist_loaded {
                                    playlist_loaded = true;

                                    // Pause GLOBAL setzen, bevor irgendetwas
                                    // angehängt wird, damit der erste Song
                                    // nicht losplappt, bevor der Sprung unten
                                    // sitzt.
                                    //
                                    // Wichtig: die Pause darf auf KEINEN Fall
                                    // per-file in die Options-Map von
                                    // `loadfile`. mpv wendet per-file Options
                                    // erneut an, sobald der Eintrag aktiv
                                    // wird — dann pausiert jeder Track die
                                    // Wiedergabe und es läuft nur der erste.
                                    let hold = serde_json::json!({
                                        "command": ["set_property", "pause", "yes"]
                                    });
                                    let _ = stream.write_all(hold.to_string().as_bytes()).await;
                                    let _ = stream.write_all(b"\n").await;

                                    for (url, title) in &playlist_clone {
                                        let cmd = loadfile_command(url, "append", title.as_deref());
                                        let _ = stream.write_all(cmd.as_bytes()).await;
                                        let _ = stream.write_all(b"\n").await;
                                    }

                                    let select = serde_json::json!({
                                        "command": ["set_property", "playlist-pos", start_index]
                                    });
                                    let _ = stream.write_all(select.to_string().as_bytes()).await;
                                    let _ = stream.write_all(b"\n").await;

                                    let resume = serde_json::json!({
                                        "command": ["set_property", "pause", "no"]
                                    });
                                    let _ = stream.write_all(resume.to_string().as_bytes()).await;
                                    let _ = stream.write_all(b"\n").await;
                                }

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
                                                // mpv meldet jeden Pause-Wechsel, auch
                                                // den aus `cycle pause` heraus.
                                                // `time-pos` friert bei Pause von
                                                // selbst ein, den Fortschritt
                                                // deshalb hier NICHT zuruecksetzen.
                                                "pause" => {
                                                    let paused = data.as_bool().unwrap_or(false);
                                                    status_clone.is_paused.store(paused, Ordering::Relaxed);
                                                    status_clone.force_ui_update.store(true, Ordering::Release);
                                                }
                                                _ => {}
                                            }
                                        }
                                    }
                                    buf.clear();
                                }
                            }
                            Err(_) => tokio::time::sleep(Duration::from_secs(1)).await,                        }
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
        self.player_status.is_paused.store(false, Ordering::Relaxed);
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
                if self.mode == ViewMode::Visualizer {
                    if let Some(fifo) = self.visualizer.fifo_path().map(|p| p.to_path_buf()) {
                        if let Some(song) = self.songs.get(current_index) {
                            let url = build_stream_url(&song.id, self.active_source, &self.config);
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

    // ── Scrobbling + local play counting ──────────────────────────────────────

    pub async fn check_and_scrobble(&mut self) {
        let current_index = self.player_status.current_index.load(Ordering::Acquire);
        if current_index == usize::MAX { return; }
        let Some(song) = self.songs.get(current_index).cloned() else { return };

        let current_time_sec   = (self.player_status.current_time.load(Ordering::Relaxed) / 1000) as u64;
        let scrobble_threshold = std::cmp::min(10, song.duration / 2);

        if current_time_sec >= scrobble_threshold
            && !self.player_status.current_scrobble_sent.load(Ordering::Acquire)
        {
            // 1. Local play count — always, source-independent
            let key = play_count_key(self.active_source, &song.id);
            *self.play_counts.entry(key).or_insert(0) += 1;
            let _ = self.save_state();

            // 2. Server scrobble — best effort, but not silent: Bandcamp's
            //    Subsonic beta rejects several of these calls, and knowing
            //    which ones is the only way to tell a real bug from an
            //    unsupported endpoint.
            let timestamp_ms = SystemTime::now()
                .duration_since(UNIX_EPOCH).unwrap().as_millis();
            if let Err(e) = scrobble(self.active_source, &song.id, timestamp_ms, &self.config).await {
                eprintln!(
                    "scrobble rejected by {}: {}",
                    self.active_source.label(),
                    e
                );
            }

            self.player_status.current_scrobble_sent.store(true, Ordering::Release);
        }
    }

    // ── Music Source Toggle ───────────────────────────────────────────────────

    pub async fn toggle_music_source(&mut self) -> Result<()> {
        let target = self.active_source.toggled();

        if !is_source_usable(target, &self.config) {
            self.status_message = match target {
                MusicSource::Navidrome => "⚠️ Navidrome is not configured".to_string(),
                MusicSource::Bandcamp if self.config.bandcamp.is_none() => {
                    "⚠️ No Bandcamp server configured in config.toml".to_string()
                }
                MusicSource::Bandcamp => {
                    "⚠️ Bandcamp is disabled or incomplete in config.toml \
                     (needs enabled = true, url, username and a token/salt or password)"
                        .to_string()
                }
            };
            return Ok(());
        }

        self.active_source = target;

        self.stop_playback().await;

        self.artists.clear();
        self.albums.clear();
        self.songs.clear();
        self.playlists.clear();
        self.search_results.clear();
        self.search_query.clear();
        self.is_search_mode = false;

        self.artist_state   = PanelState::default();
        self.album_state    = PanelState::default();
        self.song_state     = PanelState::default();
        self.playlist_state = PanelState::default();

        self.current_artist   = None;
        self.current_album    = None;
        self.current_playlist = None;
        self.mode             = ViewMode::Artists;

        self.status_message = format!("🔄 Switched to {}", self.active_source.label());

        match get_artists(self.active_source, &self.config).await {
            Ok(artists) => {
                self.artists = artists;
                if self.artists.is_empty() {
                    self.status_message =
                        format!("{} returned no artists", self.active_source.label());
                }
            }
            Err(e) => {
                self.status_message =
                    format!("❌ {}: {}", self.active_source.label(), e);
            }
        }

        match get_playlists(self.active_source, &self.config).await {
            Ok(playlists) => self.playlists = playlists,
            Err(e) => {
                self.status_message =
                    format!("❌ {} playlists: {}", self.active_source.label(), e);
            }
        }

        let _ = self.save_state();
        self.player_status.force_ui_update.store(true, Ordering::Release);
        Ok(())
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// The title mpv should display for `song`.
///
/// mpv reads its title from the ID3 tags embedded in the stream. Navidrome
/// includes them, Bandcamp does not — and without tags mpv has nothing to
/// show. The metadata is already known from the Subsonic API, so hand it to
/// mpv directly instead of relying on tags the server may not embed.
pub fn media_title(song: &Song) -> Option<String> {
    let title = song.title.trim();
    if title.is_empty() {
        return None;
    }

    let artist = song.artist.as_deref().unwrap_or("").trim();
    if artist.is_empty() {
        Some(title.to_string())
    } else {
        Some(format!("{} - {}", artist, title))
    }
}

/// A JSON-IPC `loadfile` command carrying per-file options.
///
/// `force-media-title` has to travel inside the options map: given on mpv's
/// command line it is a global option, so the last value would win for every
/// track. The options map of `loadfile` is the only way to scope a title to the
/// file it belongs to.
fn loadfile_command(url: &str, flags: &str, title: Option<&str>) -> String {
    let mut options = serde_json::Map::new();
    if let Some(title) = title {
        options.insert("force-media-title".into(), Value::String(title.to_string()));
    }

    // Hier gehören ausschliesslich echte per-file-Optionen hinein. Globale
    // Wiedergabe-Properties — insbesondere `pause` — bitte NICHT hier setzen:
    // mpv wendet die Options-Map erneut an, sobald der Eintrag aktiv wird, und
    // pausierte damit jeder weitere Song die Wiedergabe. Solche Properties
    // gehören per `set_property` gesetzt (siehe `start_playback`).
    //
    // The trailing `1` is the 1-based append position, required positionally
    // before the options map.
    serde_json::json!({
        "command": ["loadfile", url, flags, 1, Value::Object(options)]
    })
    .to_string()
}

fn play_count_key(source: MusicSource, song_id: &str) -> String {
    format!("{:?}:{}", source, song_id)
}

pub fn normalize_for_search(s: &str) -> String {
    s.to_lowercase()
}
#[cfg(test)]
mod tests {
    use super::*;

    fn song(title: &str, artist: Option<&str>) -> Song {
        Song {
            id:       "1".to_string(),
            title:    title.to_string(),
            artist:   artist.map(|a| a.to_string()),
            album:    Some("Album".to_string()),
            duration: 100,
            track:    Some(1),
            starred:  None,
        }
    }

    #[test]
    fn media_title_combines_artist_and_title() {
        assert_eq!(
            media_title(&song("Transmission", Some("Joy Division"))).as_deref(),
            Some("Joy Division - Transmission")
        );
    }

    #[test]
    fn media_title_falls_back_to_title_without_artist() {
        assert_eq!(media_title(&song("Transmission", None)).as_deref(), Some("Transmission"));
        // An artist that is present but blank should not produce a leading dash.
        assert_eq!(media_title(&song("Transmission", Some("   "))).as_deref(), Some("Transmission"));
    }

    #[test]
    fn media_title_is_none_for_an_empty_title() {
        // Without a title there is nothing better to show than mpv's own
        // fallback, so let it keep that.
        assert_eq!(media_title(&song("", Some("Joy Division"))), None);
        assert_eq!(media_title(&song("   ", None)), None);
    }

    #[test]
    fn loadfile_places_the_title_in_the_options_map() {
        let cmd = loadfile_command("http://x/stream?id=1", "append", Some("A - B"));
        let parsed: Value = serde_json::from_str(&cmd).unwrap();
        let arr = parsed["command"].as_array().unwrap();

        assert_eq!(arr[0].as_str(), Some("loadfile"));
        assert_eq!(arr[1].as_str(), Some("http://x/stream?id=1"));
        assert_eq!(arr[2].as_str(), Some("append"));
        // The 1-based append position has to sit before the options map.
        assert_eq!(arr[3].as_u64(), Some(1));

        assert_eq!(arr[4]["force-media-title"].as_str(), Some("A - B"));
    }

    #[test]
    fn loadfile_never_sets_a_global_playback_property() {
        // Regressionstest: eine per-file gesetzte `pause` wird von mpv erneut
        // angewandt, sobald der Eintrag aktiv wird. Dadurch pausierte jeder
        // Folge-Song und die Wiedergabe stoppte nach dem ersten Track. Globale
        // Properties gehoeren per `set_property` in `start_playback`.
        let cmd = loadfile_command("http://x/s", "append", Some("T"));
        let parsed: Value = serde_json::from_str(&cmd).unwrap();
        let options = parsed["command"][4].as_object().unwrap();

        assert!(!options.contains_key("pause"));
        // Grundsätzlich: die Options-Map traegt nur force-media-title.
        assert_eq!(options.len(), 1);
        assert!(options.contains_key("force-media-title"));
    }

    #[test]
    fn loadfile_without_a_title_carries_no_options() {
        let cmd = loadfile_command("http://x/s", "append", None);
        let parsed: Value = serde_json::from_str(&cmd).unwrap();
        assert!(parsed["command"][4].as_object().unwrap().is_empty());
    }

    #[test]
    fn loadfile_escapes_awkward_titles() {
        // These would break mpv's text IPC and the command line; the JSON
        // options map has to carry them intact.
        let title = r#"Zitat "hier" & Backslash \ und Komma, = Zeichen"#;
        let cmd = loadfile_command("http://x/s", "append", Some(title));
        let parsed: Value = serde_json::from_str(&cmd).unwrap();
        assert_eq!(
            parsed["command"][4]["force-media-title"].as_str(),
            Some(title)
        );
    }
}
