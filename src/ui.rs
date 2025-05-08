// src/ui.rs
use crossterm::event::{Event, KeyCode, KeyEvent};

pub struct UI {
    pub artists: Vec<String>,
    pub selected: usize,
}

impl UI {
    pub fn new(artists: Vec<String>) -> Self {
        Self {
            artists,
            selected: 0,
        }
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
                if !self.artists.is_empty() {
                    self.selected = (self.selected + 1) % self.artists.len();
                }
                None
            }
            Event::Key(KeyEvent {
                code: KeyCode::Up, ..
            }) => {
                if !self.artists.is_empty() {
                    self.selected = (self.selected + self.artists.len() - 1) % self.artists.len();
                }
                None
            }
            Event::Key(KeyEvent {
                code: KeyCode::Char('p'),
                ..
            }) => {
                if !self.artists.is_empty() {
                    Some(Action::Play(self.artists[self.selected].clone()))
                } else {
                    None
                }
            }
            _ => None,
        }
    }
}

pub enum Action {
    Quit,
    Play(String),
}