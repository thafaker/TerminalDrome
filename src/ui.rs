use crossterm::event::{Event, KeyCode, KeyEvent};
use ratatui::{
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
