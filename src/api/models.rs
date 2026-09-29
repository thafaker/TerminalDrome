#![allow(dead_code)]
use serde::{Deserialize, Serialize};

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
pub struct SubsonicResponse {
    #[serde(rename = "subsonic-response")]
    pub response: SubsonicContent,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
pub struct SubsonicContent {
    #[serde(flatten)]
    pub content: ContentType,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
#[allow(dead_code)]
pub enum ContentType {
    Artists        { artists: ArtistList },
    Albums         { artist: ArtistDetail },
    Songs          { album: AlbumDetail },
    Directory      (MusicDirectory),
    SearchResults  { #[serde(rename = "searchResult3")] search_result3: SearchResult },
    Playlists      { playlists: PlaylistList },
    PlaylistDetail { playlist: PlaylistSongs },
    RandomSongs    { #[serde(rename = "randomSongs")] random_songs: RandomSongList },
}

#[derive(Debug, Deserialize)]
pub struct ArtistList {
    pub index: Vec<ArtistGroup>,
}

#[derive(Debug, Deserialize)]
pub struct ArtistGroup {
    #[serde(default)]
    pub artist: Vec<Artist>,
}

#[derive(Debug, Deserialize, Clone, Serialize)]
pub struct Artist {
    pub id:   String,
    pub name: String,
}

#[derive(Debug, Deserialize)]
pub struct ArtistDetail {
    pub album: Vec<Album>,
}

#[derive(Debug, Deserialize, Clone, Serialize)]
pub struct Album {
    pub id:        String,
    pub name:      String,
    pub artist:    String,
    #[serde(rename = "coverArt")]
    pub cover_art: Option<String>,
    pub year:      Option<i32>,
    #[serde(rename = "songCount")]
    pub song_count: u32,
}

#[derive(Debug, Deserialize)]
pub struct AlbumDetail {
    pub song: Vec<Song>,
}

#[derive(Debug, Deserialize, Clone, Serialize)]
pub struct Song {
    pub id:       String,
    pub title:    String,
    pub duration: u64,
    pub track:    Option<u32>,
    pub artist:   Option<String>,
    pub album:    Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub starred: Option<String>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct MusicDirectory {
    pub child: Vec<Song>,
}

#[derive(Debug, Deserialize)]
pub struct SearchResult {
    pub _song: Vec<Song>,  // Unterstrich unterdrückt die Warnung
}

#[derive(Debug, Deserialize)]
pub struct RandomSongList {
    #[serde(default)]
    pub song: Vec<Song>,
}

#[derive(Debug, Deserialize, Clone, Serialize)]
pub struct Playlist {
    pub id:         String,
    pub name:       String,
    #[serde(rename = "songCount")]
    pub song_count: u32,
    #[serde(default)]
    pub duration:   u64,
    #[serde(rename = "coverArt")]
    pub cover_art:  Option<String>,
    pub comment:    Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct PlaylistList {
    pub playlist: Vec<Playlist>,
}

#[derive(Debug, Deserialize)]
pub struct PlaylistSongs {
    #[serde(default)]
    pub entry: Vec<Song>,
}

#[derive(Debug, Deserialize, Clone, Serialize)]
pub struct SongDetail {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub artist: Option<String>,
    #[serde(default)]
    pub album: Option<String>,
    #[serde(default)]
    pub duration: u64,
    #[serde(default)]
    pub track: Option<u32>,
    #[serde(default)]
    pub year: Option<i32>,
    #[serde(default)]
    pub genre: Option<String>,
    #[serde(default, rename = "bitRate")]
    pub bit_rate: Option<u32>,
    #[serde(default, rename = "contentType")]
    pub content_type: Option<String>,
    #[serde(default)]
    pub suffix: Option<String>,
    #[serde(default)]
    pub size: Option<u64>,
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub starred: Option<String>,
    #[serde(default, rename = "playCount")]
    pub play_count: Option<u64>,
}

// ── Lyrics ─────────────────────────────────────────────────────────────────

/// One timed line of a synchronised lyric.
///
/// One timed line of a lyric.
///
/// The OpenSubsonic spec puts the offset in milliseconds under `start`, and
/// Navidrome follows that: a four minute track produces values in the hundreds
/// of thousands, not a few hundred. Treating them as seconds would push every
/// line past the end of the song.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct LyricLine {
    /// Offset from the start of the track, in milliseconds.
    #[serde(rename = "start")]
    pub start_ms: u64,
    pub value: String,
}

/// The lyrics of a single track.
#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
pub struct Lyrics {
    #[serde(default)]
    pub artist: Option<String>,
    #[serde(default)]
    pub title:  Option<String>,
    /// Raw text, used when no timed line could be derived.
    #[serde(default)]
    pub value:  String,
    /// Timed lines, empty when the server sent plain unsynchronised text.
    #[serde(default)]
    pub lines:  Vec<LyricLine>,
}

impl Lyrics {
    /// Whether the server actually had lyrics for this track.
    ///
    /// Servers answer with an empty element rather than a 404 when they have
    /// nothing, so a missing payload has to be told apart from a transport
    /// error by looking at the text.
    pub fn is_empty(&self) -> bool {
        self.value.trim().is_empty() && self.lines.is_empty()
    }

    /// Index of the line that is being sung at `now_ms`, if any.
    ///
    /// The last line whose offset has already passed wins, because a line stays
    /// on screen until the next one starts rather than blinking out at its own
    /// timestamp. `parse_lrc` leaves the lines sorted by offset, which lets this
    /// be a binary search instead of a walk over every line on every frame.
    ///
    /// `None` before the first timestamp is reached, so a track that opens with
    /// instrumental music shows nothing rather than the first line too early.
    pub fn active_line(&self, now_ms: u64) -> Option<usize> {
        if self.lines.is_empty() {
            return None;
        }
        let passed = self.lines.partition_point(|l| l.start_ms <= now_ms);
        passed.checked_sub(1)
    }

    /// Build lyrics from one entry of an OpenSubsonic `structuredLyrics` list.
    ///
    /// This is the only response that carries timing, so it is the one the
    /// client asks for first. An entry without any `start` on its lines is
    /// unsynchronised and is kept as text, which is the same shape the `.lrc`
    /// parser produces for a file without timestamps.
    ///
    /// Returns `None` for an entry with no usable text, so a stray translation
    /// stub cannot blank out the real lyric.
    pub fn from_structured(entry: &serde_json::Value) -> Option<Lyrics> {
        let mut timed: Vec<LyricLine> = Vec::new();
        let mut plain: Vec<String> = Vec::new();
        for line in entry["line"].as_array()? {
            let value = line["value"].as_str().unwrap_or("").to_string();
            match line["start"].as_u64() {
                // A line with an offset is timed, whatever the `synced` flag
                // claims: the flag describes the entry, the offsets are the
                // truth about the individual line.
                Some(start_ms) => timed.push(LyricLine { start_ms, value: value.clone() }),
                None          => plain.push(value),
            }
        }
        if timed.is_empty() && plain.iter().all(|p| p.trim().is_empty()) { return None; }

        // Unsynchronised lines that sit alongside timed ones are kept in the
        // text rather than dropped, so nothing the server sent is lost.
        let mut value = plain.join("\n");
        if !timed.is_empty() {
            timed.sort_by_key(|l| l.start_ms);
            if !value.trim().is_empty() { value.push('\n'); }
            value.push_str(&timed.iter().map(|l| l.value.as_str())
                .collect::<Vec<_>>().join("\n"));
        }
        Some(Lyrics {
            artist: entry["displayArtist"].as_str().map(str::to_string),
            title:  entry["displayTitle"].as_str().map(str::to_string),
            value,
            lines: timed,
        })
    }

    /// The best entry of a `structuredLyrics` list, if there is one.
    ///
    /// A server can return several, typically a lyric plus its translation.
    /// A synchronised entry beats an unsynchronised one, because it is the only
    /// kind this client can follow along to, and within a kind the server's
    /// order wins, since that is where it puts the original.
    pub fn best_structured(list: &serde_json::Value) -> Option<Lyrics> {
        let entries: Vec<&serde_json::Value> = match list {
            serde_json::Value::Array(a) => a.iter().collect(),
            serde_json::Value::Object(_) => vec![list],
            _ => return None,
        };
        let parsed: Vec<Lyrics> = entries.iter()
            .filter_map(|e| Self::from_structured(e))
            .collect();
        parsed.iter().find(|l| !l.lines.is_empty()).or_else(|| parsed.first()).cloned()
    }

    /// Parse an `.lrc` body into timed lines.
    ///
    /// Three shapes occur in real files and all three have to survive:
    /// metadata tags such as `[ar:…]`, which describe the file rather than the
    /// song and are dropped; a single timestamp per line; and several
    /// timestamps in front of one line, which repeats the same words at
    /// different points in the track. Text with no timestamp at all is kept as
    /// untimed lines so nothing is silently lost.
    pub fn parse_lrc(body: &str) -> Vec<LyricLine> {
        let mut out = Vec::new();
        for raw in body.lines() {
            let line = raw.trim();
            if line.is_empty() { continue; }

            // Consume every leading timestamp, not just the first.
            let mut stamps: Vec<u64> = Vec::new();
            let mut rest = line;
            while let Some((ms, tail)) = Self::leading_timestamp(rest) {
                stamps.push(ms);
                rest = tail;
            }

            if stamps.is_empty() {
                if Self::is_metadata(line) { continue; }
                out.push(LyricLine { start_ms: 0, value: line.to_string() });
                continue;
            }

            for ms in stamps {
                let value = rest.trim();
                if value.is_empty() { continue; }
                out.push(LyricLine { start_ms: ms, value: value.to_string() });
            }
        }
        out.sort_by_key(|l| l.start_ms);
        out
    }

    /// Pull `[mm:ss.xx]` or `[mm:ss:xx]` off the front of a line.
    ///
    /// The fraction separator is either a dot or a colon, and the fraction is
    /// one or two digits in the files we see, so it is read as a decimal
    /// rather than parsed as a separate centisecond field.
    fn leading_timestamp(line: &str) -> Option<(u64, &str)> {
        let rest = line.strip_prefix('[')?;
        let (clock, rest) = rest.split_once(']')?;
        let mut parts = clock.split([':', '.']);
        let minutes: u64 = parts.next()?.parse().ok()?;
        let seconds: u64 = parts.next()?.parse().ok()?;
        let fraction: f64 = match parts.next() {
            Some(frac) if !frac.is_empty() => format!("0.{frac}").parse().unwrap_or(0.0),
            _ => 0.0,
        };
        let ms = minutes * 60_000 + seconds * 1000 + (fraction * 1000.0).round() as u64;
        Some((ms, rest.trim_start()))
    }

    /// Whether a line is an `.lrc` metadata tag rather than a lyric.
    ///
    /// The check is against a fixed list instead of "contains a colon" so that
    /// a real line such as `[Chorus]` is not mistaken for one.
    fn is_metadata(line: &str) -> bool {
        const TAGS: &[&str] = &["ar", "ti", "al", "au", "by", "offset", "re", "ve", "length"];

        let Some(rest) = line.strip_prefix('[') else { return false };
        let Some((key, _)) = rest.split_once(']') else { return false };
        let Some(key) = key.split([':', '.']).next() else { return false };
        TAGS.iter().any(|t| t.eq_ignore_ascii_case(key))
    }
}

#[cfg(test)]
mod lyrics_tests {
    use super::*;

    /// The shape a synced lyric actually takes in the wild.
    #[test]
    fn parses_a_real_lrc_body() {
        let body = "[ar:Radiohead]\n\
                    [ti:Creep]\n\
                    [00:19.16]When you were here before\n\
                    [00:24.09]Couldn't look you in the eye\n";
        let lines = Lyrics::parse_lrc(body);
        assert_eq!(lines.len(), 2, "metadata tags must not become lyric lines");
        assert_eq!(lines[0].value, "When you were here before");
        assert_eq!(lines[0].start_ms, 19_160);
        assert_eq!(lines[1].start_ms, 24_090);
    }

    /// Some writers emit `[mm:ss:cc]` instead of `[mm:ss.cc]`, and there are
    /// two decimals in the wild as well.
    #[test]
    fn parses_both_fraction_separators_and_widths() {
        let lines = Lyrics::parse_lrc(
            "[00:19.16]dot\n[00:24:09]colon\n[01:05.5]one decimal\n[02:00]whole seconds\n",
        );
        let got: Vec<(u64, &str)> =
            lines.iter().map(|l| (l.start_ms, l.value.as_str())).collect();
        assert_eq!(got, vec![
            (19_160, "dot"),
            (24_090, "colon"),
            (65_500, "one decimal"),
            (120_000, "whole seconds"),
        ]);
    }

    /// Minutes must not be confused with seconds.
    #[test]
    fn minutes_accumulate() {
        let lines = Lyrics::parse_lrc("[03:07.50]late\n");
        assert_eq!(lines[0].start_ms, 187_500);
    }

    /// A repeated word can carry several timestamps in front of it; both have
    /// to survive or the line is lost.
    #[test]
    fn a_line_with_two_timestamps_yields_two_lines() {
        let lines = Lyrics::parse_lrc("[00:10.00][01:10.00]chorus line\n");
        assert_eq!(lines.len(), 2, "got {lines:?}");
        assert_eq!(lines[0].start_ms, 10_000);
        assert_eq!(lines[1].start_ms, 70_000);
        assert_eq!(lines[0].value, "chorus line");
        assert_eq!(lines[1].value, "chorus line");
    }

    /// Timestamps arrive in whatever order the file happens to be written in.
    #[test]
    fn out_of_order_lines_are_sorted() {
        let lines = Lyrics::parse_lrc("[00:30.00]second\n[00:10.00]first\n");
        assert_eq!(lines[0].value, "first");
        assert_eq!(lines[1].value, "second");
    }

    /// Plain text with no timestamps at all is not LRC and must not be
    /// shredded into garbage.
    #[test]
    fn plain_text_yields_no_timed_lines() {
        let lines = Lyrics::parse_lrc("First line of a lyric\nsecond line\n");
        assert!(lines.iter().all(|l| l.start_ms == 0));
        assert_eq!(lines.len(), 2, "text must be preserved, not dropped");
    }

    /// A server with no lyrics returns an empty element, which is a normal
    /// answer and not a failure to report.
    #[test]
    fn an_empty_payload_reads_as_empty() {
        let parsed: Lyrics = serde_json::from_str(r#"{}"#).unwrap();
        assert!(parsed.is_empty());
        let absent: Lyrics = serde_json::from_value(serde_json::Value::Null)
            .unwrap_or_default();
        assert!(absent.is_empty());
    }

    /// The first line of a song is often at exactly zero; treating that as
    /// "no timestamp" would drop it.
    #[test]
    fn a_timestamp_of_zero_is_kept() {
        let lines = Lyrics::parse_lrc("[00:00.00]in the beginning\n[00:04.00]next\n");
        assert_eq!(lines.len(), 2, "got {lines:?}");
        assert_eq!(lines[0].value, "in the beginning");
        assert_eq!(lines[0].start_ms, 0);
    }

    /// Lyrics embedded in a 2000s era MP3 arrive as a USLT frame, and the
    /// three shapes below are what real files contain. The endpoint decides
    /// between timed and plain by whether the text opens with a bracket, and
    /// each shape has to survive that decision.
    #[test]
    fn embedded_lyric_shapes_survive() {
        // The most common case: plain text, no timestamps anywhere.
        let plain = "This is a plain line\nand another one";
        let lines = Lyrics::parse_lrc(plain);
        assert_eq!(lines.len(), 2, "plain text must not be lost: {lines:?}");

        // Also common: an LRC file stuffed into the USLT frame, which is what
        // the winamp era tools wrote.
        let timed = "[00:19.16]timed line here\n[00:24.09]second line";
        let lines = Lyrics::parse_lrc(timed);
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].start_ms, 19_160);
        assert_eq!(lines[1].value, "second line");

        // A section marker is not a timestamp and must survive as text rather
        // than swallowing the line behind it.
        let mixed = "[Chorus]\nhello there\n[00:05.00]later";
        let lines = Lyrics::parse_lrc(mixed);
        assert!(
            lines.iter().any(|l| l.value == "[Chorus]"),
            "the marker was eaten: {lines:?}"
        );
        assert!(lines.iter().any(|l| l.value == "hello there"), "got {lines:?}");
        assert_eq!(lines.last().unwrap().start_ms, 5_000);
    }

    /// The highlight has to know which line is being sung, and the two edges
    /// are where a naive implementation shows the wrong thing.
    #[test]
    fn active_line_tracks_playback() {
        let l = Lyrics { lines: Lyrics::parse_lrc(
            "[00:00.00]first\n[00:10.00]second\n[00:20.50]third"), ..Default::default() };

        // Before the song reaches the first lyric there is nothing to mark, so
        // an instrumental intro does not flash the opening line too early.
        assert_eq!(l.active_line(0), Some(0), "a line at 0:00 starts immediately");

        // A line stays on until the next one begins, rather than vanishing the
        // instant its own timestamp passes.
        assert_eq!(l.active_line(9_999), Some(0));
        assert_eq!(l.active_line(10_000), Some(1));
        assert_eq!(l.active_line(20_499), Some(1));
        assert_eq!(l.active_line(20_500), Some(2));

        // Past the last line the last one remains the active one for the rest
        // of the track instead of leaving the view unmarked.
        assert_eq!(l.active_line(999_999), Some(2));
    }

    /// A track that starts later than zero, and unsynchronised text, are the
    /// two cases where there is legitimately no line to highlight.
    #[test]
    fn active_line_is_absent_before_the_first_lyric() {
        let late = Lyrics { lines: Lyrics::parse_lrc("[00:30.00]only line"), ..Default::default() };
        assert_eq!(late.active_line(0), None, "nothing has been sung yet");
        assert_eq!(late.active_line(29_999), None);
        assert_eq!(late.active_line(30_000), Some(0));

        // Plain text carries no offsets at all, so there is no line to follow
        // and the view must fall back to reading it as written.
        let plain = Lyrics { value: "just words\nno timings".into(), ..Default::default() };
        assert_eq!(plain.active_line(0), None);
        assert_eq!(Lyrics::default().active_line(1_000), None);
    }

    /// Lines whose offsets arrive out of order, as a real `structuredLyrics`
    /// entry can, have to come back sorted: `active_line` binary searches them.
    #[test]
    fn structured_lines_are_sorted_by_offset() {
        let l = Lyrics::from_structured(&serde_json::json!({
            "displayTitle": "t",
            "line": [{"start": 9000, "value": "late"},
                     {"start": 1000, "value": "early"}],
        })).unwrap();
        assert_eq!(l.lines[0].value, "early");
        assert_eq!(l.active_line(2_000), Some(0), "searching a sorted list");
    }

    /// An entry that is synced but whose lines carry no offset cannot be
    /// followed, so it has to read as text rather than as a line at 0:00.
    #[test]
    fn a_synced_flag_without_offsets_is_still_just_text() {
        let l = Lyrics::from_structured(&serde_json::json!({
            "synced": true,
            "line": [{"value": "one"}, {"value": "two"}],
        })).unwrap();
        assert!(l.lines.is_empty(), "no offsets, nothing to follow");
        assert_eq!(l.value, "one\ntwo", "but the text is still there");
        assert_eq!(l.active_line(5_000), None);
    }

    /// Lines without an offset next to lines with one lose nothing: the timed
    /// part is followed, and the untimed text is still readable.
    #[test]
    fn untimed_lines_survive_next_to_timed_ones() {
        let l = Lyrics::from_structured(&serde_json::json!({
            "line": [{"value": "spoken intro"},
                     {"start": 5000, "value": "sung"},
                     {"start": 9000, "value": "sung again"}],
        })).unwrap();
        assert_eq!(l.lines.len(), 2, "the two timed lines are followed");
        assert!(l.value.contains("spoken intro"), "the text was not dropped: {}", l.value);
    }

    /// A list with several entries is how a server offers an original plus its
    /// translation. The timed one is the one worth following.
    #[test]
    fn the_best_entry_of_a_list_is_preferred() {
        let list = serde_json::json!([
            {"lang": "en", "synced": false, "line": [{"value": "plain"}]},
            {"lang": "de", "synced": true,  "line": [{"start": 4000, "value": "timed"}]},
        ]);
        let l = Lyrics::best_structured(&list).unwrap();
        assert_eq!(l.lines.len(), 1);
        assert_eq!(l.lines[0].start_ms, 4000, "timing beats list order");
    }

    /// A translation on its own still has to be shown rather than dropped.
    #[test]
    fn a_list_with_no_timed_entry_keeps_its_text() {
        let list = serde_json::json!([
            {"lang": "en", "synced": false, "line": [{"value": "english only"}]},
        ]);
        let l = Lyrics::best_structured(&list).unwrap();
        assert!(l.lines.is_empty());
        assert_eq!(l.value, "english only", "better a text than nothing");
    }

    /// Empty and malformed entries must not blank out a real lyric.
    #[test]
    fn empty_entries_are_skipped_rather_than_winning() {
        let list = serde_json::json!([
            {"synced": true, "line": []},
            {"synced": true, "line": [{"value": "  "}]},
            {"synced": true, "line": [{"start": 1000, "value": "the real one"}]},
        ]);
        let l = Lyrics::best_structured(&list).unwrap();
        assert_eq!(l.lines[0].value, "the real one", "empty entries do not win");
    }

    /// A server with nothing to offer is not a failure, and a list that is not
    /// a list at all is not a panic.
    #[test]
    fn absent_and_malformed_lists_yield_nothing() {
        assert!(Lyrics::best_structured(&serde_json::Value::Null).is_none());
        assert!(Lyrics::best_structured(&serde_json::json!([])).is_none());
        assert!(Lyrics::best_structured(&serde_json::json!("nonsense")).is_none());
        assert!(Lyrics::best_structured(&serde_json::json!({"line": "not a list"})).is_none());
    }

    /// A single entry where a list was expected must still work, the same way
    /// song lists are accepted as one bare object.
    #[test]
    fn a_bare_object_is_accepted_instead_of_a_list() {
        let l = Lyrics::best_structured(&serde_json::json!({
            "synced": true, "line": [{"start": 100, "value": "solo"}],
        })).unwrap();
        assert_eq!(l.lines[0].value, "solo");
    }

    /// The dispatch the endpoint performs: a payload that opens with a
    /// bracket is treated as timed, anything else stays plain text.
    fn dispatch(text: &str) -> Vec<LyricLine> {
        if text.trim_start().starts_with('[') { Lyrics::parse_lrc(text) } else { Vec::new() }
    }

    #[test]
    fn the_brackets_decide_timed_versus_plain() {
        assert!(dispatch("just words
no brackets").is_empty(),
                "plain text must not be reported as timed");
        assert_eq!(dispatch("[00:01.00]a").len(), 1);
        // Opening with a space must not change the decision.
        assert_eq!(dispatch("\n  [00:01.00]a").len(), 1);
    }

    #[test]
    fn non_lyric_text_is_not_mistaken_for_lrc() {
        // A line that merely starts with a bracket must not be eaten.
        let lines = Lyrics::parse_lrc("[Chorus]\nreal text\n");
        assert!(lines.iter().any(|l| l.value == "[Chorus]"), "got {lines:?}");
    }
}
