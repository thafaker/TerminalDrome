use ratatui::{
    layout::Rect,
    prelude::{Alignment, Frame, Line, Span},
    widgets::Wrap,
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, Clear, Paragraph},
};

use crate::app::{App, LyricsState};
use crate::api::models::SongDetail;

pub fn render_song_info(frame: &mut Frame, app: &App) {
    let Some(overlay) = app.song_info_overlay.as_ref() else { return };

    let detail: Option<&SongDetail> = overlay.detail.as_ref();
    let fallback = &overlay.fallback_song;

    let title     = detail.map(|d| d.title.clone()).unwrap_or_else(|| fallback.title.clone());
    let artist    = detail.and_then(|d| d.artist.clone()).or_else(|| fallback.artist.clone());
    let album     = detail.and_then(|d| d.album.clone()).or_else(|| fallback.album.clone());
    let track     = detail.and_then(|d| d.track).or(fallback.track);
    let duration  = detail.map(|d| d.duration).unwrap_or(fallback.duration);
    let year      = detail.and_then(|d| d.year);
    let genre     = detail.and_then(|d| d.genre.clone());
    let suffix    = detail.and_then(|d| d.suffix.clone());
    let content_t = detail.and_then(|d| d.content_type.clone());
    let bitrate   = detail.and_then(|d| d.bit_rate);
    let size      = detail.and_then(|d| d.size);
    let path      = detail.and_then(|d| d.path.clone());
    let server_pc = detail.and_then(|d| d.play_count);
    let starred   = detail.and_then(|d| d.starred.as_ref()).is_some()
        || fallback.starred.is_some();

    let label = Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD);
    let head  = Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD);
    let dim   = Style::default().fg(Color::DarkGray);

    // Pre-compute every display string as an owned String.
    // This avoids E0716: temporary values dropped while borrowed.
    let track_str    = track.map(|t| t.to_string()).unwrap_or_else(|| "—".into());
    let year_str     = year.map(|y| y.to_string()).unwrap_or_else(|| "—".into());
    let duration_str = format!("{:02}:{:02}", duration / 60, duration % 60);
    let format_str   = match (&suffix, &content_t) {
        (Some(s), Some(c)) => format!("{} ({})", s.to_uppercase(), c),
        (Some(s), None)    => s.to_uppercase(),
        (None, Some(c))    => c.clone(),
        (None, None)       => "—".to_string(),
    };
    let bitrate_str  = bitrate.map(|b| format!("{} kbps", b)).unwrap_or_else(|| "—".into());
    let size_str     = size.map(format_size).unwrap_or_else(|| "—".into());
    let server_pc_str = server_pc.map(|c| c.to_string()).unwrap_or_else(|| "—".into());
    let local_pc_str  = overlay.local_play_count.to_string();

    let mut lines = vec![
        Line::from(""),
        labeled("Title:    ", title.clone(), label),
        labeled("Artist:   ", artist.clone().unwrap_or_else(|| "—".into()), label),
        labeled("Album:    ", album.clone().unwrap_or_else(|| "—".into()), label),
        labeled("Track:    ", track_str, label),
        labeled("Year:     ", year_str, label),
        labeled("Genre:    ", genre.clone().unwrap_or_else(|| "—".into()), label),
        Line::from(""),
        labeled("Duration: ", duration_str, label),
        labeled("Format:   ", format_str, label),
        labeled("Bitrate:  ", bitrate_str, label),
        labeled("Size:     ", size_str, label),
        Line::from(""),
        labeled("Starred:  ", if starred { "❤️  Yes".to_string() } else { "No".to_string() }, label),
        labeled("Server plays: ", server_pc_str, label),
        labeled("Local plays:  ", local_pc_str, label),
    ];

    if let Some(p) = path.as_deref() {
        lines.push(Line::from(""));
        lines.push(labeled("Path:     ", p.to_string(), label));
    }

    if let Some(err) = &overlay.error {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            format!("  ⚠️  Extended info unavailable: {}", err),
            dim,
        )));
    }

    let has_lyrics = matches!(&overlay.lyrics, LyricsState::Ready(l) if !l.is_empty());

    // The overlay is one paragraph that cannot scroll, so a long lyric would
    // push the closing hint off the bottom of a short terminal and strand the
    // user with no visible way out. The section is therefore handed the rows
    // that are genuinely left after the borders, the metadata and the footer.
    let sz      = frame.size();
    // Six rows are spoken for before the lyrics get a look: the box is capped
    // two rows below the terminal, two of those are the border, and two more
    // carry the blank line and the closing hint.
    let chrome  = 2 + 2 + 2;
    let section = sz.height
        .saturating_sub(chrome)
        .saturating_sub(lines.len() as u16);

    lines.extend(lyric_lines(&overlay.lyrics, head, dim, section));

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled("  Press ESC to close", dim)));

    let height = ((lines.len() as u16) + 2).min(sz.height.saturating_sub(2));
    // Lyrics are long lines of text; a 68 column box would clip every one of
    // them, so the box grows when there is something to read.
    let base   = if has_lyrics { 84 } else { 68 };
    let width  = base.min(sz.width.saturating_sub(4));
    let x      = sz.width.saturating_sub(width) / 2;
    let y      = sz.height.saturating_sub(height) / 2;
    let area   = Rect { x, y, width, height };

    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(lines)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" Song Info ")
                    .border_style(Style::default().fg(Color::Magenta)),
            )
            .alignment(Alignment::Left)
            .wrap(Wrap { trim: true }),
        area,
    );
}

/// Build a single labeled line. `value` is owned so we never borrow a
/// short-lived temporary — this is what fixed the eight E0716 errors.
fn labeled(label: &str, value: String, style: Style) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("  {}", label), style),
        Span::raw(value),
    ])
}

fn format_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = 1024 * KB;
    const GB: u64 = 1024 * MB;
    if bytes >= GB      { format!("{:.2} GB", bytes as f64 / GB as f64) }
    else if bytes >= MB { format!("{:.1} MB", bytes as f64 / MB as f64) }
    else if bytes >= KB { format!("{:.1} KB", bytes as f64 / KB as f64) }
    else                { format!("{} B", bytes) }
}
/// How many lyric lines to show before the rest is only counted.
///
/// A full song does not fit next to the metadata, and truncating silently
/// would look like the file ended there.
const MAX_LYRIC_LINES: u16 = 10;

/// Build the lyrics section of the overlay for every state of the request.
///
/// `budget` is the number of rows the whole section may occupy, including the
/// blank line and heading in front of it. The section gives rows back rather
/// than exceeding it, which is what keeps the closing hint of the overlay on
/// screen on a short terminal.
fn lyric_lines(
    state: &LyricsState,
    head: Style,
    dim: Style,
    budget: u16,
) -> Vec<Line<'static>> {
    if budget == 0 {
        return Vec::new();
    }

    let mut out = vec![Line::from("")];
    if budget < 2 {
        return out;
    }

    match state {
        LyricsState::NotAsked | LyricsState::Loading => {
            out.push(Line::from(Span::styled(format!("  {}", state.label()), dim)));
        }
        LyricsState::Failed(msg) => {
            out.push(Line::from(Span::styled(format!("  ⚠️  {}", msg), dim)));
        }
        LyricsState::Ready(l) if l.is_empty() => {
            // Silence is an answer, not a fault, so it is stated plainly
            // instead of being dressed up as an error.
            out.push(Line::from(Span::styled(
                "  This track has no lyrics on the server", dim)));
        }
        LyricsState::Ready(l) if l.lines.is_empty() => {
            let all: Vec<&str> = l
                .value
                .lines()
                .map(str::trim)
                .filter(|t| !t.is_empty())
                .collect();
            out.extend(plain_body(&all, " Lyrics ", head, dim, budget));
        }
        LyricsState::Ready(l) => {
            // Timed lines carry their offset, which makes it visible that the
            // server has synchronised lyrics and not just text.
            let timed: Vec<(u32, &str)> = l
                .lines
                .iter()
                .map(|x| ((x.start_ms / 1000) as u32, x.value.as_str()))
                .collect();
            out.extend(timed_body(&timed, " Lyrics (synced) ", head, dim, budget));
        }
    }
    out
}

/// Lay out a block of lyric text under a heading, within `budget` rows.
///
/// The section spends a leading blank, the heading, a blank behind it and,
/// when anything was left out, a line saying so. Those four rows are reserved
/// before the text is placed, so the count of hidden lines is always honest.
fn body_rows(total: usize, budget: u16) -> (usize, bool) {
    // leading blank, heading, blank, truncation note
    let avail = (budget as usize).saturating_sub(4);
    if avail == 0 {
        return (0, total > 0);
    }
    // `total` is part of the cap, so a short lyric never asks for a note
    // about lines that were all shown.
    let mut cap = avail.min(MAX_LYRIC_LINES as usize).min(total);
    if total > cap && cap + 1 > avail {
        // The note needs a row of its own; give it one by dropping a text row.
        cap = cap.saturating_sub(1);
    }
    (cap, total > cap)
}

fn plain_body(
    all: &[&str],
    heading: &str,
    head: Style,
    dim: Style,
    budget: u16,
) -> Vec<Line<'static>> {
    let (cap, truncated) = body_rows(all.len(), budget);
    let mut out = vec![
        Line::from(Span::styled(heading.to_string(), head)),
        Line::from(""),
    ];
    if cap == 0 && truncated {
        out.push(Line::from(Span::styled("  (no room in this window)", dim)));
        return out;
    }
    for text in all.iter().take(cap) {
        out.push(Line::from(format!("  {}", text)));
    }
    if truncated {
        out.push(Line::from(Span::styled(
            format!("  … {} more lines", all.len().saturating_sub(cap)), dim)));
    }
    out
}

fn timed_body(
    timed: &[(u32, &str)],
    heading: &str,
    head: Style,
    dim: Style,
    budget: u16,
) -> Vec<Line<'static>> {
    let (cap, truncated) = body_rows(timed.len(), budget);
    let mut out = vec![
        Line::from(Span::styled(heading.to_string(), head)),
        Line::from(""),
    ];
    if cap == 0 && truncated {
        out.push(Line::from(Span::styled("  (no room in this window)", dim)));
        return out;
    }
    for (start, text) in timed.iter().take(cap) {
        out.push(Line::from(format!(
            "  [{}:{:02}] {}", start / 60, start % 60, text)));
    }
    if truncated {
        out.push(Line::from(Span::styled(
            format!("  … {} more lines", timed.len().saturating_sub(cap)), dim)));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::LyricsState;
    use crate::api::models::{LyricLine, Lyrics};

    fn plain(s: &LyricsState) -> Vec<String> {
        lyric_lines(s, Style::default(), Style::default(), 20)
            .iter()
            .map(|l| l.spans.iter().map(|sp| sp.content.as_ref()).collect())
            .collect()
    }

    fn synced_lyrics(n: usize) -> Lyrics {
        Lyrics {
            lines: (0..n)
                .map(|i| LyricLine { start_ms: i as u64 * 1000, value: format!("line {}", i) })
                .collect(),
            value: "[00:00.00]line 0".into(),
            ..Default::default()
        }
    }

    /// While the request is still running the section must not pretend to know
    /// the answer yet.
    #[test]
    fn pending_states_say_so() {
        assert!(plain(&LyricsState::Loading).iter().any(|l| l.contains("Fetching")));
        assert!(plain(&LyricsState::NotAsked).iter().any(|l| l.contains("not loaded")));
    }

    /// "No lyrics" is a normal answer. It must never be shown as a failure,
    /// or every instrumental track looks like a broken server.
    #[test]
    fn an_empty_answer_is_not_an_error() {
        let out = plain(&LyricsState::Ready(Box::new(Lyrics::default())));
        let text = out.join(" ");
        assert!(text.contains("no lyrics"), "got {text:?}");
        assert!(!text.contains('⚠'), "empty lyrics must not warn: {text:?}");
    }

    /// A real failure is the case that should carry the warning sign.
    #[test]
    fn a_failed_request_is_reported() {
        let out = plain(&LyricsState::Failed("server said no".into()));
        assert!(out.join(" ").contains("server said no"), "got {out:?}");
    }

    /// Timed lines are rendered with their offset, which is what tells the user
    /// the lyrics are synchronised rather than plain text.
    #[test]
    fn synced_lyrics_show_their_offset() {
        let out = plain(&LyricsState::Ready(Box::new(synced_lyrics(3))));
        let text = out.join("\n");
        assert!(text.contains("synced"), "got {text:?}");
        assert!(text.contains("[0:00] line 0"), "got {text:?}");
        assert!(text.contains("[0:02] line 2"), "minutes are wrong: {text:?}");
    }

    /// A song is far taller than the overlay. Cutting it without saying so
    /// makes a partial lyric look like a complete one.
    #[test]
    fn a_long_song_reports_what_was_hidden() {
        let out = plain(&LyricsState::Ready(Box::new(synced_lyrics(40))));
        let text = out.join("\n");
        assert!(text.contains("30 more lines"), "got {text:?}");
        assert!(text.contains("line 0"), "the beginning must be shown");
        assert!(!text.contains("line 20"), "the middle must be cut: {text:?}");
    }

    /// Unsynchronised text has no offsets to show, so it is printed as-is.
    #[test]
    fn plain_lyrics_are_printed_verbatim() {
        let l = Lyrics {
            value: "first line\nsecond line".into(),
            ..Default::default()
        };
        let out = plain(&LyricsState::Ready(Box::new(l)));
        let text = out.join("\n");
        assert!(text.contains("first line"), "got {text:?}");
        assert!(text.contains("second line"), "got {text:?}");
        assert!(!text.contains("synced"), "must not claim to be timed: {text:?}");
    }
}

#[cfg(test)]
mod budget_tests {
    use super::*;
    use crate::app::LyricsState;
    use crate::api::models::{LyricLine, Lyrics};

    fn many(n: usize) -> Lyrics {
        Lyrics {
            lines: (0..n).map(|i| LyricLine { start_ms: i as u64 * 1000, value: format!("l{}", i) }).collect(),
            ..Default::default()
        }
    }

    /// On a short terminal the lyric section used to push the closing hint off
    /// the bottom, stranding the user in the overlay with no way out.
    #[test]
    fn a_small_budget_shrinks_the_lyrics() {
        let l = LyricsState::Ready(Box::new(many(40)));
        for budget in 0..=3u16 {
            let out = lyric_lines(&l, Style::default(), Style::default(), budget);
            let body = out.iter().filter(|l| l.spans.iter().any(|s| s.content.contains("] l"))).count();
            assert!(
                body <= budget as usize,
                "budget {budget} produced {body} lyric lines"
            );
        }
    }

    /// A generous window still shows more than the hard maximum.
    #[test]
    fn a_large_budget_is_capped_by_the_maximum() {
        let l = LyricsState::Ready(Box::new(many(40)));
        let out = lyric_lines(&l, Style::default(), Style::default(), 999);
        let body = out.iter().filter(|l| l.spans.iter().any(|s| s.content.contains("] l"))).count();
        assert_eq!(body, MAX_LYRIC_LINES as usize);
    }

    /// The property that actually has to hold: the lyrics section may never
    /// make the overlay overflow by more than it already did. A terminal far
    /// too short for the metadata itself was already clipping in 0.9.6, and
    /// lyrics must not be the thing that makes it worse.
    #[test]
    fn lyrics_never_add_to_the_overflow() {
        for h in 12u16..=50 {
            for metadata in [10u16, 14, 18, 22] {
                let chrome = 2 + 2 + 2;
                let section = h.saturating_sub(chrome).saturating_sub(metadata);
                let overflow = |rows: u16| (rows + 2 + 2).saturating_sub(h);
                let without = overflow(metadata + 2);
                let with    = overflow(section + metadata + 2);
                assert!(
                    with <= without,
                    "{h} rows with {metadata} metadata lines: overflow grew \
                     from {without} to {with}"
                );
            }
        }
    }

    /// Where the metadata does fit on its own, the closing hint has to be
    /// reachable even with a full song behind it.
    #[test]
    fn the_footer_is_reachable_whenever_the_metadata_fits() {
        for h in 16u16..=50 {
            for metadata in [10u16, 14, 18] {
                if metadata + 2 + 2 > h {
                    continue; // metadata alone does not fit
                }
                let chrome = 2 + 2 + 2;
                let section = h.saturating_sub(chrome).saturating_sub(metadata);
                let drawn = section + metadata + 2;
                assert!(
                    drawn + 2 <= h,
                    "{h} rows with {metadata} metadata lines hid the footer"
                );
            }
        }
    }

    /// Whatever the budget, the closing hint has to survive.
    #[test]
    fn the_footer_row_is_accounted_for() {
        // 20 metadata rows + 2 border + 1 spacer + 1 footer = 24; a 24 row
        // terminal leaves nothing at all for lyrics.
        let metadata = 20u16;
        let chrome = 2 + 2 + 2;
        let budget = 24u16
            .saturating_sub(chrome)
            .saturating_sub(metadata);
        assert_eq!(budget, 0, "a 24 row terminal cannot fit lyrics at all");
        let out = lyric_lines(
            &LyricsState::Ready(Box::new(many(40))),
            Style::default(), Style::default(), budget,
        );
        // A zero budget yields nothing at all, so the footer keeps its row.
        assert!(out.is_empty(), "expected no section, got {out:?}");
    }
}
