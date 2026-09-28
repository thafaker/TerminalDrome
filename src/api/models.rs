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
/// The legacy `getLyrics` response carries timestamps in milliseconds under
/// `start`, while `getLyricsBySongId` uses whole seconds. We normalise to
/// milliseconds here so callers never have to know which server answered.
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

    #[test]
    fn non_lyric_text_is_not_mistaken_for_lrc() {
        // A line that merely starts with a bracket must not be eaten.
        let lines = Lyrics::parse_lrc("[Chorus]\nreal text\n");
        assert!(lines.iter().any(|l| l.value == "[Chorus]"), "got {lines:?}");
    }
}
