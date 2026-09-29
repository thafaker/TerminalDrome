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
///
/// The time comes off the overlay rather than off the player, because the
/// overlay is what decides to stop following on a song change. Reading the
/// player here would let the highlight keep moving through the old words.
pub fn render_lyrics(frame: &mut Frame, o: &LyricsOverlay) {

    let sz    = frame.size();
    let head  = Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD);
    let dim   = Style::default().fg(Color::DarkGray);
    let text  = Style::default();
    // Lines already sung are dimmed so the eye lands on the current one.
    let sung  = Style::default().fg(Color::DarkGray);
    let now_s = Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD);

    let heading = match &o.artist {
        Some(a) => format!(" {} — {} ", o.title, a),
        None    => format!(" {} ", o.title),
    };

    let total     = o.state.row_count_at(sz.width);
    let body_rows = sz.height.saturating_sub(CHROME).max(1);
    let first     = o.scroll.min(total.saturating_sub(1));
    let shown     = total.saturating_sub(first).min(body_rows);

    // Which source line is being sung, so the view can mark it. Only timed
    // lyrics have an answer, and only while playback is on the same track.
    let active = o.state.active_index(o.now_ms);

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
            // where a line falls without it competing with the words. The
            // line being sung is marked so the text can be read while it
            // plays rather than hunted for.
            Text::from(
                l.lines
                    .iter()
                    .enumerate()
                    .map(|(i, x)| {
                        let sec = (x.start_ms / 1000) as u32;
                        let (stamp, body) = match active {
                            Some(a) if a == i => (now_s, now_s),
                            Some(a) if a > i  => (sung, sung),
                            _                 => (dim, text),
                        };
                        Line::from(vec![
                            Span::styled(
                                format!("  {:>3}:{:02}  ", sec / 60, sec % 60), stamp),
                            Span::styled(x.value.clone(), body),
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
    // Saying that the view has stopped following is better than a page that
    // sits still and looks stuck. It goes in the hint line because the title
    // bar already carries the position.
    let block = block.title_bottom(Line::from(Span::styled(
        if o.follow {
            " ↑↓ PgUp/PgDn scroll · Home/End · ESC close "
        } else {
            " ↑↓ PgUp/PgDn scroll · Home/End · follow off · ESC close "
        },
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
    use ratatui::{backend::TestBackend, Terminal};

    fn timed(n: usize) -> Lyrics {
        Lyrics {
            lines: (0..n)
                .map(|i| LyricLine { start_ms: i as u64 * 3000, value: format!("line {}", i) })
                .collect(),
            value: String::new(),
            ..Default::default()
        }
    }

    fn overlay(l: Lyrics) -> LyricsOverlay {
        LyricsOverlay {
            title: "song".into(),
            artist: Some("artist".into()),
            state: LyricsState::Ready(Box::new(l)),
            scroll: 0,
            song_id: "id".into(),
            now_ms: 0,
            follow: true,
        }
    }

    /// Set the clock the way the tick does, then render to a string one line
    /// per row, so a test can look at what a terminal would actually show.
    ///
    /// The clock is set directly rather than through `follow_playback`, so that
    /// a test about highlighting still works after the view has been scrolled
    /// by hand and has stopped following.
    fn drawn(o: &mut LyricsOverlay, now_ms: u64, w: u16, h: u16) -> String {
        o.now_ms = now_ms;
        let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
        term.draw(|f| render_lyrics(f, o)).unwrap();
        let buf = term.backend().buffer().clone();
        (0..h)
            .map(|y| (0..w).map(|x| buf.get(x, y).symbol()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// The style of one cell, so a test can tell a highlighted line from a
    /// dimmed one without reading the text.
    fn style_of(o: &mut LyricsOverlay, now_ms: u64, x: u16, y: u16) -> Style {
        o.now_ms = now_ms;
        let mut term = Terminal::new(TestBackend::new(60, 20)).unwrap();
        term.draw(|f| render_lyrics(f, o)).unwrap();
        term.backend().buffer().get(x, y).style()
    }

    /// The row count is what the scroll position is clamped against, so paging
    /// past the end must not leave an empty view.
    #[test]
    fn scrolling_never_runs_past_the_end() {
        let state = LyricsState::Ready(Box::new(timed(100)));
        assert_eq!(state.row_count_at(80), 100);
        // A caller asking for row 500 must land on the last full screen.
        let visible = 22u16;
        let max = state.row_count_at(80) - visible;
        assert_eq!(max, 78);
    }

    /// Plain text has no timestamps, so the row count comes from the lines
    /// themselves and a lyric of blank lines still counts as content.
    #[test]
    fn plain_text_reports_its_own_length() {
        let l = Lyrics { value: "one\n\ntwo\nthree".into(), ..Default::default() };
        assert_eq!(LyricsState::Ready(Box::new(l)).row_count_at(80), 3);
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
            assert_eq!(s.row_count_at(80), 1, "{s:?}");
        }
    }

    /// A line longer than the box occupies several screen rows, and the view
    /// has to count them, or following a line would scroll to the wrong place.
    #[test]
    fn wrapped_lines_count_the_rows_they_occupy() {
        // 60 columns of box leave 58 for text, and the timestamp takes ten.
        let long = "x".repeat(120);
        let text_cols = long.chars().count() + LyricsState::TIMESTAMP_COLS;
        let l = Lyrics { lines: vec![LyricLine { start_ms: 0, value: long }], ..Default::default() };
        let state = LyricsState::Ready(Box::new(l));

        // 60 columns of box leave 58 for text, and the timestamp takes ten.
        let expected = (text_cols as u16).div_ceil(58); // 58 columns of room
        assert_eq!(state.row_count_at(60), expected);
        assert!(expected > 2, "a 120 character lyric should wrap: {expected}");

        // The offset of a line past the wrapped one has to include the rows it
        // took, which is the whole reason the count is not just a line count.
        let two = Lyrics { lines: vec![
            LyricLine { start_ms: 0, value: "x".repeat(120) },
            LyricLine { start_ms: 1_000, value: "short".into() },
        ], ..Default::default() };
        assert_eq!(
            LyricsState::Ready(Box::new(two)).row_offset_at(60, 1),
            expected,
            "the second line starts below the wrapped first one",
        );
    }

    /// The timestamp prefix in the view has to be exactly as wide as the
    /// constant the row count assumes, or every wrapped lyric is measured
    /// short by however many columns the two disagree on.
    #[test]
    fn the_timestamp_width_is_the_one_the_view_draws() {
        for (min, sec) in [(0u32, 0u32), (0, 9), (1, 5), (4, 59), (61, 23)] {
            let drawn = format!("  {:>3}:{:02}  ", min, sec);
            assert_eq!(
                drawn.chars().count(),
                LyricsState::TIMESTAMP_COLS,
                "timestamp for {min}:{sec} is {} columns wide",
                drawn.chars().count(),
            );
        }
    }

    /// The whole point of the feature: the line being sung has to look
    /// different from the ones already past and the ones still to come.
    #[test]
    fn the_sung_line_is_marked() {
        let mut o = overlay(timed(4));
        // line 0 at 0:00, line 1 at 0:03, line 2 at 0:06.
        let view = drawn(&mut o, 6_500, 60, 12);
        assert!(view.contains("line 2"), "the active line is on screen: {view}");

        let active  = style_of(&mut o, 6_500, 12, 3);
        let before  = style_of(&mut o, 6_500, 12, 2);
        assert_ne!(active, before, "sung line differs from the one before it");
        assert!(
            active.add_modifier.contains(Modifier::BOLD),
            "the current line is the emphasised one",
        );
    }

    /// Before the first timestamp there is nothing to mark, and after the last
    /// one the last line stays on rather than the view going blank.
    #[test]
    fn the_mark_follows_playback_and_settles_at_the_end() {
        let l = Lyrics { lines: Lyrics::parse_lrc(
            "[00:05.00]first\n[00:10.00]second"), ..Default::default() };
        let o = overlay(l);
        assert_eq!(o.state.active_index(0), None, "nothing sung yet");
        assert_eq!(o.state.active_index(5_000), Some(0));
        assert_eq!(o.state.active_index(60_000), Some(1), "last line stays");
    }

    /// A page the reader scrolled to by hand has to stay where they put it,
    /// and the view has to admit that it is no longer following.
    #[test]
    fn a_hand_scrolled_page_says_it_is_not_following() {
        let mut o = overlay(timed(40));
        assert!(o.follow);

        o.scroll_by(10, 80, 22);
        assert!(!o.follow, "scrolling by hand stops the follow");
        assert_eq!(o.scroll, 10, "and the page stays where it was put");

        // Playback carrying on must not drag the view back.
        o.follow_playback(30_000, 80, 22, true);
        assert_eq!(o.scroll, 10, "a hand-scrolled page is left alone");

        let view = drawn(&mut o, 5_000, 80, 25);
        assert!(view.contains("follow off"), "the view admits it: {view}");

        o.scroll_home();
        assert!(o.follow, "Home resumes following");
        assert_eq!(o.scroll, 0);
    }

    /// A long song scrolls itself down as the lines pass, one screenful at a
    /// time, and never past the end of the text.
    #[test]
    fn the_view_scrolls_itself_as_the_song_plays() {
        let mut o = overlay(timed(100));
        let (w, h) = (80u16, 22u16);

        // The first lines fit on screen, so nothing moves yet.
        o.follow_playback(0, w, h, true);
        assert_eq!(o.scroll, 0, "no scroll while the first lines are visible");

        // Play far enough that the sung line has gone off the bottom. Lines are
        // three seconds apart, so line 30 is a minute and a half in, well past
        // the 22 rows on screen.
        o.follow_playback(90_000, w, h, true);
        let active = 30usize;
        assert!(o.scroll > 0, "the view followed playback: {}", o.scroll);
        let top = o.state.row_offset_at(w, active);
        let bottom = o.state.row_offset_at(w, active + 1) - 1;
        assert!(top >= o.scroll && bottom < o.scroll + h,
                "line {active} rows {top}-{bottom} outside window at {}", o.scroll);

        // A line comfortably inside the window must not drag the page along.
        o.scroll = 20; // window 20..41, with the sung line 31 in the middle
        o.follow_playback(93_000, w, h, true);
        assert_eq!(o.scroll, 20, "no creeping while the line is visible");

        // Near the end it stops at the last screenful instead of running off.
        o.follow_playback(600_000, w, h, true);
        assert_eq!(o.scroll, o.state.row_count_at(w) - h, "clamped at the end");
    }

    /// While the lyrics belong to the previous track, playback moving on must
    /// not walk the old words forward.
    #[test]
    fn lyrics_of_another_track_do_not_follow() {
        let mut o = overlay(timed(100));
        o.follow_playback(60_000, 80, 22, false);
        assert_eq!(o.scroll, 0, "a song change leaves the page alone");

        // The clock has to freeze too, not just the scroll. The old words stay
        // on screen, so a highlight that kept moving would walk through them
        // with the time of a song they have nothing to do with.
        assert_eq!(o.now_ms, 0, "the clock stopped on the song change");
        assert_eq!(o.state.active_index(o.now_ms), Some(0), "and so the line did");

        // Still hand scrollable, so a reader who wants to read the old text is
        // not locked out of it by the song having changed.
        o.scroll_by(5, 80, 22);
        assert_eq!(o.scroll, 5);
        assert!(!o.follow, "which is a hand scroll, and it says so");

        // And when the same track comes back, it picks up from the live clock.
        o.scroll_home();
        o.follow_playback(90_000, 80, 22, true);
        assert_eq!(o.now_ms, 90_000);
        assert!(o.scroll > 0, "and follows again to the line that is now singing");
    }
}
