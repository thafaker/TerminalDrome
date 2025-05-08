use anyhow::Result;
use api::{NavidromeClient, Artist, Album, Song};
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, List, ListItem, ListState},
    Frame, Terminal,
};
use std::{io, time::Duration};

mod api;
mod audio;
mod config;
mod ui;

#[derive(Debug)]
enum CurrentView {
    Artists,
    Albums,
    Songs,
}

struct App {
    client: NavidromeClient,
    artists: Vec<Artist>,
    albums: Vec<Album>,
    songs: Vec<Song>,
    current_view: CurrentView,
    artist_state: ListState,
    album_state: ListState,
    song_state: ListState,
    audio_player: audio::AudioPlayer,
}

impl App {
    async fn new(config: &config::AppConfig) -> Result<Self> {
        let client = NavidromeClient::new(config)?;
        let artists = client.get_artists().await?;

        Ok(Self {
            client,
            artists,
            albums: Vec::new(),
            songs: Vec::new(),
            current_view: CurrentView::Artists,
            artist_state: ListState::default(),
            album_state: ListState::default(),
            song_state: ListState::default(),
            audio_player: audio::AudioPlayer::new(config),
        })
    }

    async fn load_albums(&mut self, artist_index: usize) -> Result<()> {
        if let Some(artist) = self.artists.get(artist_index) {
            self.albums = self.client.get_albums(&artist.id).await?;
            self.current_view = CurrentView::Albums;
            self.album_state.select(Some(0));
        }
        Ok(())
    }

    async fn load_songs(&mut self, album_index: usize) -> Result<()> {
        if let Some(album) = self.albums.get(album_index) {
            self.songs = self.client.get_songs(&album.id).await?;
            self.current_view = CurrentView::Songs;
            self.song_state.select(Some(0));
        }
        Ok(())
    }

    fn play_song(&mut self, song_index: usize) {
        if let Some(song) = self.songs.get(song_index) {
            let stream_url = self.client.get_play_url(&song.id);
            self.audio_player.play_song(&stream_url);
        }
    }

    fn draw(&mut self, f: &mut Frame) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .margin(1)
            .constraints([Constraint::Min(1)].as_slice())
            .split(f.size());

        match self.current_view {
            CurrentView::Artists => {
                let items: Vec<ListItem> = self
                    .artists
                    .iter()
                    .map(|a| ListItem::new(format!("{} ({})", a.name, a.album_count)))
                    .collect();

                let list = List::new(items)
                    .block(Block::default().borders(Borders::ALL).title("Artists"))
                    .highlight_style(Style::default().bg(Color::DarkGray).add_modifier(Modifier::BOLD));

                f.render_stateful_widget(list, chunks[0], &mut self.artist_state);
            }
            CurrentView::Albums => {
                let items: Vec<ListItem> = self
                    .albums
                    .iter()
                    .map(|a| ListItem::new(format!("{} - {} ({})", a.artist, a.name, a.song_count)))
                    .collect();

                let list = List::new(items)
                    .block(Block::default().borders(Borders::ALL).title("Albums (ESC to go back)"))
                    .highlight_style(Style::default().bg(Color::DarkGray).add_modifier(Modifier::BOLD));

                f.render_stateful_widget(list, chunks[0], &mut self.album_state);
            }
            CurrentView::Songs => {
                let items: Vec<ListItem> = self
                    .songs
                    .iter()
                    .map(|s| ListItem::new(format!("{} - {}", s.artist, s.title)))
                    .collect();

                let list = List::new(items)
                    .block(Block::default().borders(Borders::ALL).title("Songs (ESC to go back)"))
                    .highlight_style(Style::default().bg(Color::DarkGray).add_modifier(Modifier::BOLD));

                f.render_stateful_widget(list, chunks[0], &mut self.song_state);
            }
        }
    }

    async fn handle_input(&mut self, event: Event) -> Result<()> {
        match event {
            Event::Key(KeyEvent {
                code: KeyCode::Char('q'),
                ..
            }) => return Ok(()),
            Event::Key(KeyEvent {
                code: KeyCode::Esc, ..
            }) => match self.current_view {
                CurrentView::Albums => self.current_view = CurrentView::Artists,
                CurrentView::Songs => self.current_view = CurrentView::Albums,
                _ => {}
            },
            Event::Key(KeyEvent {
                code: KeyCode::Down,
                ..
            }) => self.move_selection(1),
            Event::Key(KeyEvent {
                code: KeyCode::Up, ..
            }) => self.move_selection(-1),
            Event::Key(KeyEvent {
                code: KeyCode::Enter,
                ..
            }) => self.handle_enter().await?,
            Event::Key(KeyEvent {
                code: KeyCode::Char(' '),
                ..
            }) => {
                if let CurrentView::Songs = self.current_view {
                    if let Some(selected) = self.song_state.selected() {
                        self.play_song(selected);
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn move_selection(&mut self, delta: i32) {
        let (state, items) = match self.current_view {
            CurrentView::Artists => (&mut self.artist_state, self.artists.len()),
            CurrentView::Albums => (&mut self.album_state, self.albums.len()),
            CurrentView::Songs => (&mut self.song_state, self.songs.len()),
        };

        if items > 0 {
            let selected = state.selected().unwrap_or(0);
            let new_selected = (selected as i32 + delta).rem_euclid(items as i32) as usize;
            state.select(Some(new_selected));
        }
    }

    async fn handle_enter(&mut self) -> Result<()> {
        match self.current_view {
            CurrentView::Artists => {
                if let Some(selected) = self.artist_state.selected() {
                    self.load_albums(selected).await?;
                }
            }
            CurrentView::Albums => {
                if let Some(selected) = self.album_state.selected() {
                    self.load_songs(selected).await?;
                }
            }
            CurrentView::Songs => {
                if let Some(selected) = self.song_state.selected() {
                    self.play_song(selected);
                }
            }
        }
        Ok(())
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    // Terminal Setup
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // App Setup
    let config = config::AppConfig::load()?;
    let mut app = App::new(&config).await?;
    app.artist_state.select(Some(0));

    // Main Loop
    loop {
        terminal.draw(|f| app.draw(f))?;

        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                if key.code == KeyCode::Char('q') {
                    break;
                }
                app.handle_input(Event::Key(key)).await?;
            }
        }
    }

    // Cleanup
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    Ok(())
}