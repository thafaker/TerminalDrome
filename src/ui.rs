use crossterm::event::{Event, KeyCode, KeyEvent};
use ratatui::{
    backend::Backend,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, List, ListItem, ListState},
    Frame,
};

pub struct UI {
    pub artists: Vec<String>,
    pub songs: Vec<String>,
    pub selected_artist: usize,
    pub selected_song: usize,
    pub in_song_view: bool,
    pub list_state: ListState,
}

impl UI {
    pub fn new(artists: Vec<String>) -> Self {
        let mut list_state = ListState::default();
        list_state.select(Some(0));
        
        Self {
            artists,
            songs: Vec::new(),
            selected_artist: 0,
            selected_song: 0,
            in_song_view: false,
            list_state,
        }
    }

    pub fn handle_input(&mut self, event: Event) -> Option<Action> {
        match event {
            Event::Key(KeyEvent {
                code: KeyCode::Char('q'),
                ..
            }) => Some(Action::Quit),
            Event::Key(KeyEvent {
                code: KeyCode::Esc, ..
            }) if self.in_song_view => {
                self.in_song_view = false;
                None
            }
            Event::Key(KeyEvent {
                code: KeyCode::Down,
                ..
            }) => {
                self.move_selection(1);
                None
            }
            Event::Key(KeyEvent {
                code: KeyCode::Up, ..
            }) => {
                self.move_selection(-1);
                None
            }
            Event::Key(KeyEvent {
                code: KeyCode::Enter,
                ..
            }) => {
                if self.in_song_view {
                    Some(Action::PlaySong(self.selected_song))
                } else {
                    Some(Action::SelectArtist(self.selected_artist))
                }
            }
            _ => None,
        }
    }

    fn move_selection(&mut self, delta: i32) {
        let items = if self.in_song_view {
            &self.songs
        } else {
            &self.artists
        };

        if !items.is_empty() {
            let selected = if self.in_song_view {
                &mut self.selected_song
            } else {
                &mut self.selected_artist
            };

            *selected = (*selected as i32 + delta).rem_euclid(items.len() as i32) as usize;
            self.list_state.select(Some(*selected));
        }
    }

    pub fn draw(&mut self, f: &mut Frame) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .margin(1)
            .constraints([Constraint::Min(1)].as_slice())
            .split(f.size());

        let items = if self.in_song_view {
            &self.songs
        } else {
            &self.artists
        };

        let list_items: Vec<ListItem> = items
            .iter()
            .map(|item| ListItem::new(item.clone()))
            .collect();

        let list = List::new(list_items)
            .block(Block::default().borders(Borders::ALL).title(if self.in_song_view {
                "Songs (ESC to go back)"
            } else {
                "Artists"
            }))
            .highlight_style(
                Style::default()
                    .bg(Color::DarkGray)
                    .add_modifier(Modifier::BOLD),
            );

        f.render_stateful_widget(list, chunks[0], &mut self.list_state);
    }
}

pub enum Action {
    Quit,
    SelectArtist(usize),
    PlaySong(usize),
}
