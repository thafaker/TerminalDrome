use crossterm::event::{Event, KeyCode, KeyEvent};

pub struct UI {
    pub artists: Vec<String>,
    pub selected: usize,
}

impl UI {
    pub fn new(artists: Vec<String>) -> Self {
        Self { artists, selected: 0 }
    }

    pub fn handle_input(&mut self, event: Event) -> Option<Action> {
        match event {
            Event::Key(KeyEvent {
                code: KeyCode::Char('q'),
                ..
            }) => Some(Action::Quit),
            Event::Key(KeyEvent {
                code: KeyCode::Down,
                ..
            }) => {
                self.selected = (self.selected + 1) % self.artists.len();
                None
            }
            Event::Key(KeyEvent {
                code: KeyCode::Up, ..
            }) => {
                self.selected = (self.selected + self.artists.len() - 1) % self.artists.len();
                None
            }
            Event::Key(KeyEvent {
                code: KeyCode::Char('p'),
                ..
            }) => Some(Action::Play(self.artists[self.selected].clone())),
            _ => None,
        }
    }
}

pub enum Action {
    Quit,
    Play(String),
}
