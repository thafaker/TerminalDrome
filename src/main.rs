use anyhow::Result;
use api::{Artist, NavidromeClient};
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, List, ListItem, ListState},
    Terminal,
};
use std::{io, time::Duration};

mod api;
mod audio;
mod config;

struct App {
    client: NavidromeClient,
    artists: Vec<Artist>,
    artist_state: ListState,
}

impl App {
    async fn new(config: &config::AppConfig) -> Result<Self> {
        let client = NavidromeClient::new(config)?;
        let artists = client.get_artists().await?;

        Ok(Self {
            client,
            artists,
            artist_state: ListState::default(),
        })
    }

    fn draw(&mut self, f: &mut ratatui::Frame) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .margin(1)
            .constraints([Constraint::Min(1)].as_slice())
            .split(f.size());

        let items: Vec<ListItem> = self
            .artists
            .iter()
            .map(|a| ListItem::new(a.name.clone()))
            .collect();

        let list = List::new(items)
            .block(Block::default().borders(Borders::ALL).title("Artists"))
            .highlight_style(Style::default().bg(Color::DarkGray).add_modifier(Modifier::BOLD));

        f.render_stateful_widget(list, chunks[0], &mut self.artist_state);
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let config = config::AppConfig::load()?;
    let mut app = App::new(&config).await?;
    app.artist_state.select(Some(0));

    loop {
        terminal.draw(|f| app.draw(f))?;

        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                match key.code {
                    KeyCode::Char('q') => break,
                    KeyCode::Down => app.move_selection(1),
                    KeyCode::Up => app.move_selection(-1),
                    _ => {}
                }
            }
        }
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

impl App {
    fn move_selection(&mut self, delta: i32) {
        if !self.artists.is_empty() {
            let selected = self.artist_state.selected().unwrap_or(0);
            let new_selected = (selected as i32 + delta).rem_euclid(self.artists.len() as i32) as usize;
            self.artist_state.select(Some(new_selected));
        }
    }
}