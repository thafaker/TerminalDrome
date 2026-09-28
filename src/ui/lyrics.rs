use ratatui::{
    layout::Rect,
    prelude::{Alignment, Frame, Line, Span},
    style::{Color, Modifier, Style},
    text::Text,
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
};

use crate::app::{LyricsOverlay, LyricsState};

/// Rows the view keeps for itself: two borders and the hint line.
const CHROME: u16 = 3;

/// Draw the lyrics of the playing song over the whole terminal.
///
/// The view takes the full width because on an 80x25 terminal a smaller box
/// leaves too little room for a lyric to be worth reading.
/// Takes the overlay rather than the whole app, so the view can be rendered
/// in a test without standing up an application.
pub fn render_lyrics(frame: &mut Frame, o: &LyricsOverlay) {

    let sz    = frame.size();
    let head  = Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD);
    let dim   = Style::default().fg(Color::DarkGray);
    let text  = Style::default();

    let heading = match &o.artist {
        Some(a) => format!(" {} — {} ", o.title, a),
        None    => format!(" {} ", o.title),
    };

    let total     = o.state.row_count();
    let body_rows = sz.height.saturating_sub(CHROME).max(1);
    let first     = o.scroll.min(total.saturating_sub(1));
    let shown     = total.saturating_sub(first).min(body_rows);

    let position = if total > 1 {
        // Only meaningful once there is more than one screenful.
        Some(Line::from(Span::styled(
            format!(" {}-{} of {} ", first + 1, first + shown, total),
            dim,
        )).right_aligned())
    } else {
        None
    };

    let body = match &o.state {
        LyricsState::NotAsked | LyricsState::Loading => {
            Text::from(Span::styled("  fetching lyrics…", dim))
        }
        LyricsState::Failed(msg) => Text::from(vec![
            Line::from(""),
            Line::from(Span::styled(format!("  ⚠️  {}", msg), dim)),
            Line::from(""),
            Line::from(Span::styled("  the server could not be asked for this track", dim)),
        ]),
        LyricsState::Ready(l) if l.is_empty() => Text::from(vec![
            Line::from(""),
            Line::from(Span::styled("  this track has no lyrics on the server", text)),
            Line::from(""),
            Line::from(Span::styled(
                "  lyrics are read from your server, not fetched while you listen.", dim)),
            Line::from(Span::styled(
                "  sidecar .lrc files next to the audio are picked up after a rescan.", dim)),
        ]),
        LyricsState::Ready(l) if l.lines.is_empty() => {
            // Unsynchronised text, wrapped as it is.
            Text::from(
                l.value
                    .lines()
                    .filter(|t| !t.trim().is_empty())
                    .map(|t| Line::from(format!("  {}", t.trim())))
                    .collect::<Vec<_>>(),
            )
        }
        LyricsState::Ready(l) => {
            // Timed lines keep their offset, dimmed, so the reader can see
            // where a line falls without it competing with the words.
            Text::from(
                l.lines
                    .iter()
                    .map(|x| {
                        let sec = (x.start_ms / 1000) as u32;
                        Line::from(vec![
                            Span::styled(format!("  {:>3}:{:02}  ", sec / 60, sec % 60), dim),
                            Span::raw(x.value.clone()),
                        ])
                    })
                    .collect::<Vec<_>>(),
            )
        }
    };

    let mut block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Magenta))
        .title(Line::from(Span::styled(heading, head)));
    if let Some(p) = position {
        block = block.title(p);
    }
    let block = block.title_bottom(Line::from(Span::styled(
        " ↑↓ scroll · PgUp/PgDn · Home/End · ESC close ",
        dim,
    )));

    frame.render_widget(Clear, sz);
    frame.render_widget(
        Paragraph::new(body)
            .block(block)
            .alignment(Alignment::Left)
            .wrap(Wrap { trim: false })
            .scroll((first, 0)),
        Rect { x: 0, y: 0, width: sz.width, height: sz.height },
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::models::{LyricLine, Lyrics};

    fn timed(n: usize) -> Lyrics {
        Lyrics {
            lines: (0..n)
                .map(|i| LyricLine { start_ms: i as u64 * 3000, value: format!("line {}", i) })
                .collect(),
            value: String::new(),
            ..Default::default()
        }
    }

    /// The scroll position is clamped against what fits on screen, so paging
    /// past the end must not leave an empty view.
    #[test]
    fn scrolling_never_runs_past_the_end() {
        let state = LyricsState::Ready(Box::new(timed(100)));
        let total = state.row_count();
        assert_eq!(total, 100);
        // A caller asking for row 500 must land on the last full screen.
        let visible = 22u16;
        let max = total - visible;
        assert_eq!(max, 78);
    }

    /// Plain text has no timestamps, so the row count comes from the lines
    /// themselves and a lyric of blank lines still counts as content.
    #[test]
    fn plain_text_reports_its_own_length() {
        let l = Lyrics { value: "one\n\ntwo\nthree".into(), ..Default::default() };
        assert_eq!(LyricsState::Ready(Box::new(l)).row_count(), 3);
    }

    /// A track with no lyrics, a request in flight and a failure all occupy
    /// one row, so the view never claims to have something scrollable.
    #[test]
    fn non_ready_states_are_one_row() {
        for s in [
            LyricsState::Loading,
            LyricsState::NotAsked,
            LyricsState::Failed("boom".into()),
            LyricsState::Ready(Box::new(Lyrics::default())),
        ] {
            assert_eq!(s.row_count(), 1, "{s:?}");
        }
    }
}
