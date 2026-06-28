use ratatui::{
    layout::Rect,
    prelude::Frame,
    style::{Color, Style},
    widgets::{Block, Borders, Paragraph},
};
use crate::app::App;

pub fn render_search_input(frame: &mut Frame, app: &App) {
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
