use crate::api::models::{Album, Artist, Lyrics, Playlist, Song, SongDetail};
use crate::config::{Config, ServerConfig};
use anyhow::{anyhow, bail, Context, Result};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MusicSource {
    Navidrome,
    Bandcamp,
}

impl MusicSource {
    pub fn label(self) -> &'static str {
        match self {
            MusicSource::Navidrome => "Navidrome",
            MusicSource::Bandcamp  => "Bandcamp",
        }
    }

    pub fn toggled(self) -> Self {
        match self {
            MusicSource::Navidrome => MusicSource::Bandcamp,
            MusicSource::Bandcamp  => MusicSource::Navidrome,
        }
    }
}

/// True when `source` is switched on and carries usable credentials.
///
/// The main server is always considered usable — `main` already validates it
/// during startup. For the optional `[bandcamp]` section this is the single
/// source of truth, shared by the footer hint, the `Shift+B` toggle and the
/// state restore, so those three can no longer disagree.
pub fn is_source_usable(source: MusicSource, config: &Config) -> bool {
    match source {
        MusicSource::Navidrome => true,
        MusicSource::Bandcamp  => config
            .bandcamp
            .as_ref()
            .map_or(false, |bc| bc.is_configured()),
    }
}

/// Credentials for a source.
///
/// Falls back to the main server when Bandcamp is absent, so this never
/// panics. Callers must gate on [`is_source_usable`] first, otherwise a stale
/// `active_source` would silently query Navidrome while the UI claims Bandcamp.
pub fn get_target_config(source: MusicSource, config: &Config) -> &ServerConfig {
    match source {
        MusicSource::Navidrome => &config.server,
        MusicSource::Bandcamp  => config.bandcamp.as_ref().unwrap_or(&config.server),
    }
}

/// Salt for configs that store a pre-computed token but no password.
///
/// Subsonic validates `t` against the salt it receives, so a stored token can
/// only be replayed with the salt it was built from. This constant is kept
/// solely so such a config keeps working the way it always has; the
/// password path below never uses it.
const LEGACY_FALLBACK_SALT: &str = "c5a6b7";

/// A fresh random salt, as the Subsonic auth scheme requires: the client picks
/// a new salt per request and sends `t = md5(password + salt)`.
fn fresh_salt() -> String {
    use rand::Rng;
    const ALPHABET: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789";
    let mut rng = rand::thread_rng();
    (0..12)
        .map(|_| ALPHABET[rng.gen_range(0..ALPHABET.len())] as char)
        .collect()
}

pub fn build_auth_query_for_source(
    source: MusicSource,
    config: &Config,
) -> Vec<(&'static str, String)> {
    let target = get_target_config(source, config);
    let mut params = vec![
        ("u", target.username.clone()),
        ("v", "1.16.1".to_string()),
        ("c", "terminaldrome".to_string()),
        ("f", "json".to_string()),
    ];

    // Preferred: a password lets us mint a correctly salted token per request.
    if let Some(pass) = target.password.as_deref().filter(|p| !p.is_empty()) {
        let salt = fresh_salt();
        let digest = md5::compute(format!("{}{}", pass, salt));
        params.push(("t", format!("{:x}", digest)));
        params.push(("s", salt));
    } else if let (Some(token), Some(salt)) = (&target.token, &target.salt) {
        // Legacy path: a token/salt pair computed once at setup time. Still
        // valid, but it never rotates, so treat it as a fallback.
        params.push(("t", token.clone()));
        params.push(("s", salt.clone()));
    } else if let Some(token) = target.token.as_deref().filter(|t| !t.is_empty()) {
        // FUCK FUCK FUC
        // Token without salt: nothing sensible to send, but keep the request
        // shaped correctly so the server answers with a proper error instead of
        // us failing with a hand-rolled message.
        params.push(("t", token.to_string()));
        params.push(("s", LEGACY_FALLBACK_SALT.to_string()));
    }

    params
}

// ── Response handling ─────────────────────────────────────────────────────────

fn truncate(body: &str) -> String {
    let body = body.trim();
    if body.chars().count() <= 200 {
        return body.to_string();
    }
    let head: String = body.chars().take(200).collect();
    format!("{}...", head)
}

/// Expand a Subsonic error code into something a human can act on.
/// See the error table at https://www.subsonic.org/pages/api.jsp
fn describe_error_code(code: i64, message: &str) -> String {
    let text = match code {
        0  if message.is_empty() => "unspecified error".to_string(),
        0  => message.to_string(),
        10 => "required parameter missing".to_string(),
        20 => "client too old — the server wants a newer API version".to_string(),
        30 => "server rejected the requested API version".to_string(),
        40 => "wrong username or password/token".to_string(),
        41 => "server does not support token authentication".to_string(),
        42 => "server accepts no supported authentication method".to_string(),
        43 => "conflicting authentication parameters".to_string(),
        44 => "malformed authentication token".to_string(),
        50 => "not authorized for this resource".to_string(),
        60 => "trial period expired".to_string(),
        70 => "not found".to_string(),
        _  if message.is_empty() => format!("error code {}", code),
        _  => format!("error code {} ({})", code, message),
    };

    if code != 0 && !message.is_empty() && !text.contains(message) {
        format!("{}: {}", text, message)
    } else {
        text
    }
}

/// Reject a Subsonic envelope that reports `status = "failed"`.
///
/// A missing `status` is tolerated so that slightly off-spec servers keep
/// working; an explicit failure is never tolerated.
fn check_subsonic_status(sub: &Value, endpoint: &str) -> Result<()> {
    if sub.get("status").and_then(Value::as_str) != Some("failed") {
        return Ok(());
    }

    let err = sub.get("error");
    let code = err
        .and_then(|e| e.get("code"))
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let message = err
        .and_then(|e| e.get("message"))
        .and_then(Value::as_str)
        .unwrap_or("");

    bail!("{}: {}", endpoint, describe_error_code(code, message))
}

/// Run one Subsonic REST call and return its `subsonic-response` object.
///
/// Every endpoint goes through here. That matters because a broken endpoint must
/// surface as an error rather than as an empty list: Bandcamp's Subsonic
/// support is a partial beta that answers unsupported or broken endpoints with
/// HTTP 200 and a body which either has no `subsonic-response` at all or carries
/// `status = "failed"`. Indexing such a body blind yields `Ok(vec![])` — an
/// empty collection that is indistinguishable from a real one.
async fn call(
    source: MusicSource,
    config: &Config,
    endpoint: &str,
    extra: &[(&str, String)],
) -> Result<Value> {
    let target = get_target_config(source, config);
    let mut params = build_auth_query_for_source(source, config);
    params.extend_from_slice(extra);

    let url = format!("{}/rest/{}", target.url.trim_end_matches('/'), endpoint);

    let response = Client::new()
        .get(&url)
        .query(&params)
        .send()
        .await
        .with_context(|| format!("{} could not be reached at {}", endpoint, target.url))?;

    let status = response.status();
    let body = response
        .text()
        .await
        .with_context(|| format!("{}: could not read the response body", endpoint))?;

    if !status.is_success() {
        bail!("{}: HTTP {}: {}", endpoint, status, truncate(&body));
    }

    let json: Value = serde_json::from_str(&body).with_context(|| {
        format!(
            "{}: expected a JSON response, got: {}",
            endpoint,
            truncate(&body)
        )
    })?;

    let sub = json.get("subsonic-response").ok_or_else(|| {
        anyhow!(
            "{}: response is not a Subsonic response: {}",
            endpoint,
            truncate(&body)
        )
    })?;

    check_subsonic_status(sub, endpoint)?;
    Ok(sub.clone())
}

fn song_from_json(s: &Value) -> Option<Song> {
    let id = s["id"].as_str()?;
    let title = s["title"].as_str()?;
    Some(Song {
        id:       id.to_string(),
        title:    title.to_string(),
        artist:   s["artist"].as_str().map(|v| v.to_string()),
        album:    s["album"].as_str().map(|v| v.to_string()),
        duration: s["duration"].as_u64().unwrap_or(0),
        track:    s["track"].as_u64().map(|t| t as u32),
        starred:  s["starred"].as_str().map(|v| v.to_string()),
    })
}

/// A `song` list is normally a JSON array, but some servers collapse a
/// single-element list down to a bare object. Accept both so a one-track album
/// does not come back empty.
fn songs_from_json(list: &Value) -> Vec<Song> {
    match list {
        Value::Array(arr) => arr.iter().filter_map(song_from_json).collect(),
        Value::Object(_)   => song_from_json(list).into_iter().collect(),
        _                 => Vec::new(),
    }
}

// ── Browsing ──────────────────────────────────────────────────────────────────

pub async fn get_artists(source: MusicSource, config: &Config) -> Result<Vec<Artist>> {
    let sub = call(source, config, "getArtists.view", &[]).await?;

    let mut artists = Vec::new();
    if let Some(index_list) = sub["artists"]["index"].as_array() {
        for idx in index_list {
            if let Some(artist_list) = idx["artist"].as_array() {
                for a in artist_list {
                    if let (Some(id), Some(name)) = (a["id"].as_str(), a["name"].as_str()) {
                        artists.push(Artist {
                            id:   id.to_string(),
                            name: name.to_string(),
                        });
                    }
                }
            }
        }
    }
    Ok(artists)
}

pub async fn get_playlists(source: MusicSource, config: &Config) -> Result<Vec<Playlist>> {
    let sub = call(source, config, "getPlaylists.view", &[]).await?;

    let mut playlists = Vec::new();
    if let Some(list) = sub["playlists"]["playlist"].as_array() {
        for p in list {
            if let (Some(id), Some(name)) = (p["id"].as_str(), p["name"].as_str()) {
                playlists.push(Playlist {
                    id:         id.to_string(),
                    name:       name.to_string(),
                    comment:    p["comment"].as_str().map(|s| s.to_string()),
                    song_count: p["songCount"].as_u64().unwrap_or(0) as u32,
                    duration:   p["duration"].as_u64().unwrap_or(0),
                    cover_art:  p["coverArt"].as_str().map(|s| s.to_string()),
                });
            }
        }
    }
    Ok(playlists)
}

pub async fn get_artist_albums(
    source: MusicSource,
    artist_id: &str,
    config: &Config,
) -> Result<Vec<Album>> {
    let sub = call(
        source,
        config,
        "getArtist.view",
        &[("id", artist_id.to_string())],
    )
    .await?;

    let mut albums = Vec::new();
    if let Some(list) = sub["artist"]["album"].as_array() {
        for a in list {
            if let (Some(id), Some(name)) = (
                a["id"].as_str().or_else(|| a["title"].as_str()),
                a["name"].as_str().or_else(|| a["title"].as_str()),
            ) {
                albums.push(Album {
                    id:         id.to_string(),
                    name:       name.to_string(),
                    artist:     a["artist"].as_str().unwrap_or("Unknown").to_string(),
                    year:       a["year"].as_i64().map(|y| y as i32),
                    song_count: a["songCount"].as_u64().unwrap_or(0) as u32,
                    cover_art:  a["coverArt"].as_str().map(|s| s.to_string()),
                });
            }
        }
    }
    Ok(albums)
}

pub async fn get_album_songs(
    source: MusicSource,
    album_id: &str,
    config: &Config,
) -> Result<Vec<Song>> {
    let sub = call(
        source,
        config,
        "getAlbum.view",
        &[("id", album_id.to_string())],
    )
    .await?;

    Ok(songs_from_json(&sub["album"]["song"]))
}

pub async fn get_playlist_songs(
    source: MusicSource,
    playlist_id: &str,
    config: &Config,
) -> Result<Vec<Song>> {
    let sub = call(
        source,
        config,
        "getPlaylist.view",
        &[("id", playlist_id.to_string())],
    )
    .await?;

    Ok(songs_from_json(&sub["playlist"]["entry"]))
}

pub async fn get_random_songs(
    source: MusicSource,
    config: &Config,
    size: usize,
) -> Result<Vec<Song>> {
    let sub = call(
        source,
        config,
        "getRandomSongs.view",
        &[("size", size.to_string())],
    )
    .await?;

    Ok(songs_from_json(&sub["randomSongs"]["song"]))
}

// ── Searching ─────────────────────────────────────────────────────────────────

pub async fn search_songs(
    source: MusicSource,
    query: &str,
    config: &Config,
) -> Result<Vec<Song>> {
    // `query` is always sent, even when empty: some servers (Bandcamp among
    // them) only accept the call if the parameter is present with its `=`.
    let sub = call(
        source,
        config,
        "search3.view",
        &[
            ("query", query.to_string()),
            ("songCount", "50".to_string()),
        ],
    )
    .await?;

    Ok(songs_from_json(&sub["searchResult3"]["song"]))
}

// ── Annotation ────────────────────────────────────────────────────────────────

pub async fn star_song(source: MusicSource, song_id: &str, config: &Config) -> Result<()> {
    call(
        source,
        config,
        "star.view",
        &[("id", song_id.to_string())],
    )
    .await?;
    Ok(())
}

pub async fn scrobble(
    source: MusicSource,
    song_id: &str,
    timestamp_ms: u128,
    config: &Config,
) -> Result<()> {
    call(
        source,
        config,
        "scrobble.view",
        &[
            ("id", song_id.to_string()),
            ("time", timestamp_ms.to_string()),
            ("submission", "true".to_string()),
        ],
    )
    .await?;
    Ok(())
}

pub async fn get_song_info(
    source: MusicSource,
    song_id: &str,
    config: &Config,
) -> Result<SongDetail> {
    let sub = call(
        source,
        config,
        "getSong.view",
        &[("id", song_id.to_string())],
    )
    .await?;

    if sub["song"].is_null() {
        bail!("getSong.view: the server returned no song data for id {}", song_id);
    }

    serde_json::from_value(sub["song"].clone())
        .with_context(|| format!("getSong.view: unexpected song payload for id {}", song_id))
}

// ── Playlist management ───────────────────────────────────────────────────────

/// Create a new playlist with an optional initial song.
///
/// The Subsonic API accepts either `playlistId` (update) or `name` (create).
/// We use the "create" path with `name` plus an optional `songId` to seed it
/// in one request.
pub async fn create_playlist(
    source: MusicSource,
    name: &str,
    initial_song_id: Option<&str>,
    config: &Config,
) -> Result<String> {
    let mut extra = vec![("name", name.to_string())];
    if let Some(song_id) = initial_song_id {
        extra.push(("songId", song_id.to_string()));
    }

    let sub = call(source, config, "createPlaylist.view", &extra).await?;

    // The response may include the newly created playlist id under
    // `playlist.id`. Some servers omit it, so we return an empty string in that
    // case — the caller can refetch the playlist list.
    Ok(sub["playlist"]["id"].as_str().unwrap_or("").to_string())
}

/// Add a song to an existing playlist.
pub async fn add_song_to_playlist(
    source: MusicSource,
    playlist_id: &str,
    song_id: &str,
    config: &Config,
) -> Result<()> {
    call(
        source,
        config,
        "updatePlaylist.view",
        &[
            ("playlistId", playlist_id.to_string()),
            ("songIdToAdd", song_id.to_string()),
        ],
    )
    .await?;
    Ok(())
}

/// Remove a song from a playlist by its *index* within that playlist.
///
/// The Subsonic API identifies playlist entries by position, not by song id,
/// because the same song can appear multiple times in one playlist.
pub async fn remove_song_from_playlist(
    source: MusicSource,
    playlist_id: &str,
    song_index: usize,
    config: &Config,
) -> Result<()> {
    call(
        source,
        config,
        "updatePlaylist.view",
        &[
            ("playlistId", playlist_id.to_string()),
            ("songIndexToRemove", song_index.to_string()),
        ],
    )
    .await?;
    Ok(())
}

/// Delete a whole playlist.
#[allow(dead_code)]
pub async fn delete_playlist(
    source: MusicSource,
    playlist_id: &str,
    config: &Config,
) -> Result<()> {
    call(
        source,
        config,
        "deletePlaylist.view",
        &[("id", playlist_id.to_string())],
    )
    .await?;
    Ok(())
}

// ── Tests ─────────────────────────────────────────────────────────────────────

// ── Lyrics ─────────────────────────────────────────────────────────────────

/// Fetch lyrics for a track.
///
/// Uses the legacy `getLyrics` call with `artist` and `title` rather than the
/// newer `getLyricsBySongId`, which is an OpenSubsonic extension that plain
/// Subsonic servers (Bandcamp among them) do not implement. Both Navidrome
/// and Navidrome-compatible servers answer the legacy form, and Navidrome
/// still packs its `.lrc` timestamps into it, so synced lyrics survive the
/// choice.
///
/// A track with no lyrics is **not** an error: the server replies with an
/// empty element, and the caller gets `Ok` with `is_empty()` set. That
/// distinction matters, because "this track has no lyrics" is a normal state
/// and not a failure to report to the user.
pub async fn get_lyrics(
    source: MusicSource,
    song_id: &str,
    artist: &str,
    title: &str,
    config: &Config,
) -> Result<Lyrics> {
    // The endpoint that carries timing is asked for first. The legacy one below
    // cannot be replaced outright, because it is the only one every Subsonic
    // server answers, so it stays as the fallback rather than the primary.
    match get_structured_lyrics(source, song_id, config).await {
        Ok(l) if !l.is_empty() => return Ok(l),
        // A server that does not know the endpoint, or a track it has nothing
        // for, is not an error. The legacy path gets its turn.
        _ => {}
    }

    // The API matches on artist and title, not on id; song_id is only used for
    // the error message, which is where a wrong id would actually be noticed.
    let _ = song_id;

    let sub = call(
        source,
        config,
        "getLyrics.view",
        &[
            ("artist", artist.to_string()),
            ("title",  title.to_string()),
        ],
    )
    .await?;

    let node = &sub["lyrics"];
    if node.is_null() {
        return Ok(Lyrics::default());
    }

    let text = node["value"].as_str().unwrap_or("").to_string();
    let lines = if text.trim_start().starts_with('[') {
        Lyrics::parse_lrc(&text)
    } else {
        Vec::new()
    };

    Ok(Lyrics {
        artist: node["artist"].as_str().map(str::to_string),
        title:  node["title"].as_str().map(str::to_string),
        value:  text,
        lines,
    })
}

/// The `getLyricsBySongId` response, which is the only one that keeps the
/// timestamps of a synced lyric.
///
/// Deliberately returns `Ok(empty)` rather than an error for a response that
/// simply holds nothing, because "this track has no lyrics" and "this server
/// does not know the endpoint" have to lead the caller to the same place, which
/// is the legacy endpoint.
async fn get_structured_lyrics(
    source: MusicSource,
    song_id: &str,
    config: &Config,
) -> Result<Lyrics> {
    let sub = match call(source, config, "getLyricsBySongId", &[("id", song_id.to_string())]).await {
        Ok(sub) => sub,
        // An endpoint the server does not have, or one it refuses, is not a
        // failure of the request: the caller has a second way to ask.
        Err(_) => return Ok(Lyrics::default()),
    };
    Ok(Lyrics::best_structured(&sub["lyricsList"]["structuredLyrics"])
        .unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    const SYNCED: &str = r#"{"subsonic-response":{"status":"ok","lyricsList":
        {"structuredLyrics":[{"displayArtist":"a","displayTitle":"t","lang":"xxx",
        "synced":true,"line":[
          {"start":10970,"value":"first"},
          {"start":34370,"value":"second"}]}]}}}"#;

    const PLAIN_LEGACY: &str = r#"{"subsonic-response":{"status":"ok",
        "lyrics":{"artist":"a","title":"t","value":"no timings here"}}}"#;

    // The point of the whole change. Navidrome's legacy endpoint answers with
    // the words and no timings, so a client that only asks that one can never
    // follow a line along. The structured endpoint is the one carrying `start`.
    #[tokio::test]
    async fn structured_lyrics_win_because_they_are_the_only_ones_with_timings() {
        let s = stub_routes(vec![("getLyricsBySongId", "200 OK", SYNCED)]).await;
        let l = get_lyrics(MusicSource::Navidrome, "id", "a", "t", &config_for(&s.url))
            .await.unwrap();
        assert_eq!(l.lines.len(), 2, "two timed lines came through");
        assert_eq!(l.lines[0].start_ms, 10_970, "milliseconds, not seconds");
        assert_eq!(l.lines[1].value, "second");
        assert_eq!(s.endpoints(), vec!["/rest/getLyricsBySongId"],
                   "no reason to ask the legacy endpoint once timing arrived");
    }

    // A server without the OpenSubsonic endpoint must keep working. This is the
    // compatibility the fallback exists for, and it is the case a naive
    // "replace the endpoint" fix would break for every other user.
    #[tokio::test]
    async fn a_server_without_the_structured_endpoint_still_returns_lyrics() {
        let s = stub_routes(vec![
            ("getLyricsBySongId", "404 Not Found", r#"{"subsonic-response":{"status":"failed"}}"#),
            ("getLyrics.view", "200 OK", PLAIN_LEGACY),
        ]).await;
        let l = get_lyrics(MusicSource::Navidrome, "id", "a", "t", &config_for(&s.url))
            .await.unwrap();
        assert_eq!(l.value, "no timings here", "the legacy text still arrives");
        assert!(l.lines.is_empty(), "and carries no timing, as expected");
        assert_eq!(s.endpoints(), vec!["/rest/getLyricsBySongId", "/rest/getLyrics.view"],
                   "it asked the timing endpoint first, then fell back");
    }

    // An endpoint the server does not recognise must not surface as an error
    // the user sees, only as a quiet step to the next way of asking.
    #[tokio::test]
    async fn a_missing_endpoint_is_not_an_error() {
        let s = stub_routes(vec![
            ("getLyricsBySongId", "500 Internal Server Error", "boom"),
            ("getLyrics.view", "200 OK", PLAIN_LEGACY),
        ]).await;
        let l = get_lyrics(MusicSource::Navidrome, "id", "a", "t", &config_for(&s.url))
            .await
            .expect("an unsupported endpoint is not a failure the user should see");
        assert_eq!(l.value, "no timings here");
    }

    // A track with neither must be empty, not an error. Servers answer with an
    // empty element rather than a 404 when they have nothing.
    #[tokio::test]
    async fn a_track_without_lyrics_is_empty_on_both_paths() {
        let s = stub_routes(vec![
            ("getLyricsBySongId", "200 OK", r#"{"subsonic-response":{"status":"ok","lyricsList":{}}}"#),
            ("getLyrics.view", "200 OK", r#"{"subsonic-response":{"status":"ok"}}"#),
        ]).await;
        let l = get_lyrics(MusicSource::Navidrome, "id", "a", "t", &config_for(&s.url))
            .await.unwrap();
        assert!(l.is_empty(), "no lyrics anywhere: {l:?}");
    }

    // Both endpoints empty and the legacy one unreachable is a real error and
    // has to say so, rather than leaving the user with a blank panel.
    #[tokio::test]
    async fn a_real_failure_is_reported() {
        let s = stub_routes(vec![("", "200 OK", r#"{"subsonic-response":{"status":"failed",
            "error":{"code":40,"message":"Wrong username or password"}}}"#)]).await;
        let err = get_lyrics(MusicSource::Navidrome, "id", "a", "t", &config_for(&s.url))
            .await
            .expect_err("bad credentials must not look like a track without lyrics");
        assert!(format!("{:#}", err).contains("wrong username or password"), "{:#}", err);
    }

    // A translation and an original arrive in one list. Following the lyric
    // means picking the one with timings, not the one that came first.
    #[tokio::test]
    async fn a_synced_entry_beats_an_unsynced_one_in_the_same_list() {
        let s = stub_routes(vec![("getLyricsBySongId", "200 OK", r#"{"subsonic-response":
            {"status":"ok","lyricsList":{"structuredLyrics":[
              {"lang":"en","synced":false,"line":[{"value":"plain first in the list"}]},
              {"lang":"de","synced":true,"line":[{"start":5000,"value":"timed"}]}]}}}"#)]).await;
        let l = get_lyrics(MusicSource::Navidrome, "id", "a", "t", &config_for(&s.url))
            .await.unwrap();
        assert_eq!(l.lines.len(), 1, "the timed one was chosen");
        assert_eq!(l.lines[0].start_ms, 5000);
    }

    /// Canned HTTP response plus the request line the server actually saw, so
    /// tests can assert on what went out over the wire.
    struct Stub {
        url: String,
        /// Every request line the stub saw, in order.
        seen: Arc<Mutex<Vec<String>>>,
    }

    impl Stub {
        /// The request lines seen so far, as endpoint paths without the query.
        fn endpoints(&self) -> Vec<String> {
            self.seen.lock().unwrap().iter().map(|r| {
                r.split_whitespace().nth(1).unwrap_or("")
                    .split('?').next().unwrap_or("").to_string()
            }).collect()
        }
    }

    /// Serve a single canned response on an ephemeral port.
    async fn stub(status_line: &'static str, body: &'static str) -> Stub {
        stub_routes(vec![("", status_line, body)]).await
    }

    /// Serve several canned responses, picked by what the request line names.
    ///
    /// Answering more than one request is what the lyrics fallback needs, since
    /// the client is expected to ask one endpoint, see nothing usable, and then
    /// ask the other. A catch-all route is a `("", ...)` entry.
    async fn stub_routes(routes: Vec<(&'static str, &'static str, &'static str)>) -> Stub {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&seen);

        tokio::spawn(async move {
            // One connection per request, for as many as the client makes.
            while let Ok((mut socket, _)) = listener.accept().await {
                let mut buf = [0u8; 4096];
                let n = match socket.read(&mut buf).await { Ok(n) => n, Err(_) => break };
                let request = String::from_utf8_lossy(&buf[..n]).to_string();
                sink.lock().unwrap().push(request.clone());
                let (status_line, body) = routes.iter()
                    .find(|(name, _, _)| request.contains(name))
                    .map(|(_, s, b)| (*s, *b))
                    .or_else(|| routes.last().map(|(_, s, b)| (*s, *b)))
                    .unwrap_or(("404 Not Found", "{}"));
                let response = format!(
                    "HTTP/1.1 {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\
                     Connection: close\r\n\r\n{}",
                    status_line,
                    body.len(),
                    body
                );
                let _ = socket.write_all(response.as_bytes()).await;
                let _ = socket.flush().await;
            }
        });

        Stub { url: format!("http://{}", addr), seen }
    }

    fn config_for(url: &str) -> Config {
        Config {
            server: ServerConfig {
                enabled:  true,
                url:      url.to_string(),
                username: "joe".to_string(),
                password: Some("sesame".to_string()),
                token:    None,
                salt:     None,
            },
            bandcamp: None,
        }
    }

    const OK_ARTISTS: &str = r#"{"subsonic-response":{"status":"ok",
        "artists":{"index":[{"artist":[{"id":"1","name":"Aha"}]}]}}}"#;

    // The whole point of the fix: Bandcamp answers broken endpoints with HTTP
    // 200. That must surface as an error, not as an empty artist list.
    #[tokio::test]
    async fn failed_envelope_is_an_error_not_an_empty_list() {
        let body = r#"{"subsonic-response":{"status":"failed",
            "error":{"code":40,"message":"Wrong username or password"}}}"#;
        let s = stub("200 OK", body).await;
        let err = get_artists(MusicSource::Navidrome, &config_for(&s.url))
            .await
            .expect_err("a failed envelope must not look like an empty result");
        assert!(format!("{:#}", err).contains("wrong username or password"), "{:#}", err);
    }

    // Bandcamp also returns 200 with a body that is not a Subsonic envelope.
    #[tokio::test]
    async fn non_subsonic_body_is_an_error() {
        let s = stub("200 OK", r#"{"error":"bad version"}"#).await;
        let err = get_artists(MusicSource::Navidrome, &config_for(&s.url))
            .await
            .expect_err("a non-Subsonic body must not look like an empty result");
        assert!(format!("{:#}", err).contains("not a Subsonic response"), "{:#}", err);
    }

    #[tokio::test]
    async fn http_error_is_reported() {
        let s = stub("500 Internal Server Error", "boom").await;
        let err = get_artists(MusicSource::Navidrome, &config_for(&s.url))
            .await
            .expect_err("HTTP 500 must not look like an empty result");
        assert!(format!("{:#}", err).contains("HTTP 500"), "{:#}", err);
    }

    #[tokio::test]
    async fn html_error_page_is_reported() {
        let s = stub("200 OK", "<html>gateway timeout</html>").await;
        let err = get_artists(MusicSource::Navidrome, &config_for(&s.url))
            .await
            .expect_err("an HTML body must not look like an empty result");
        assert!(format!("{:#}", err).contains("expected a JSON response"), "{:#}", err);
    }

    #[tokio::test]
    async fn successful_response_still_parses() {
        let s = stub("200 OK", OK_ARTISTS).await;
        let artists = get_artists(MusicSource::Navidrome, &config_for(&s.url)).await.unwrap();
        assert_eq!(artists.len(), 1);
        assert_eq!(artists[0].name, "Aha");
    }

    // An empty but successful collection is still a success, not an error.
    #[tokio::test]
    async fn genuinely_empty_collection_is_not_an_error() {
        let body = r#"{"subsonic-response":{"status":"ok","artists":{"index":[]}}}"#;
        let s = stub("200 OK", body).await;
        let artists = get_artists(MusicSource::Navidrome, &config_for(&s.url)).await.unwrap();
        assert!(artists.is_empty());
    }

    // Bandcamp only accepts search3 when `query` is present, even if empty.
    #[tokio::test]
    async fn search_always_sends_the_query_parameter() {
        let s = stub("200 OK", r#"{"subsonic-response":{"status":"ok"}}"#).await;
        let _ = search_songs(MusicSource::Navidrome, "", &config_for(&s.url)).await;
        let seen = s.seen.lock().unwrap().join("\n");
        assert!(seen.contains("query=&"), "{seen}");
    }

    #[test]
    fn song_lists_accept_arrays_and_bare_objects() {
        let arr = serde_json::json!([{ "id": "1", "title": "One" }]);
        assert_eq!(songs_from_json(&arr).len(), 1);

        let obj = serde_json::json!({ "id": "2", "title": "Two" });
        assert_eq!(songs_from_json(&obj).len(), 1);

        assert!(songs_from_json(&serde_json::Value::Null).is_empty());
    }

    #[test]
    fn disabled_or_incomplete_bandcamp_is_not_usable() {
        let mut config = config_for("https://example.test");
        let mut bc = ServerConfig {
            enabled:   true,
            url:       "https://bandcamp.com/api/subsonic".to_string(),
            username:  "user".to_string(),
            password:  Some("pw".to_string()),
            token:     None,
            salt:      None,
        };

        bc.enabled = false;
        config.bandcamp = Some(bc.clone());
        assert!(!is_source_usable(MusicSource::Bandcamp, &config));

        bc.enabled = true;
        config.bandcamp = Some(bc.clone());
        assert!(is_source_usable(MusicSource::Bandcamp, &config));

        // Placeholders must not count as configured.
        bc.username = "your_username".to_string();
        config.bandcamp = Some(bc.clone());
        assert!(!is_source_usable(MusicSource::Bandcamp, &config));

        config.bandcamp = None;
        assert!(!is_source_usable(MusicSource::Bandcamp, &config));
    }

    #[test]
    fn password_path_uses_a_fresh_salt_per_request() {
        let config = config_for("https://example.test");
        let first = build_auth_query_for_source(MusicSource::Navidrome, &config);
        let second = build_auth_query_for_source(MusicSource::Navidrome, &config);

        let pick = |p: &Vec<(&'static str, String)>, k: &str| {
            p.iter().find(|(key, _)| *key == k).map(|(_, v)| v.clone())
        };

        let salt_a = pick(&first, "s").expect("salt");
        let salt_b = pick(&second, "s").expect("salt");
        assert_ne!(salt_a, salt_b, "the salt must not be reused between requests");
        assert!(salt_a.len() >= 6);

        // t must be md5(password + salt) for that request's salt.
        let token_a = pick(&first, "t").expect("token");
        let expected = format!("{:x}", md5::compute(format!("sesame{}", salt_a)));
        assert_eq!(token_a, expected);
    }

    #[test]
    fn legacy_token_pair_is_still_replayed_unchanged() {
        let mut config = config_for("https://example.test");
        config.server.password = None;
        config.server.token = Some("deadbeef".to_string());
        config.server.salt = Some("abc123".to_string());

        let params = build_auth_query_for_source(MusicSource::Navidrome, &config);
        let pick = |k: &str| {
            params.iter().find(|(key, _)| *key == k).map(|(_, v)| v.clone())
        };
        assert_eq!(pick("t").as_deref(), Some("deadbeef"));
        assert_eq!(pick("s").as_deref(), Some("abc123"));
    }

    #[test]
    fn stream_url_percent_encodes_the_song_id() {
        let config = config_for("https://example.test");
        let url = crate::api::build_stream_url("tr ck/1&x=2", MusicSource::Navidrome, &config);

        // `query_pairs_mut` uses form encoding, so a space becomes `+`. What
        // matters is that no separator survives unencoded and can be mistaken
        // for the end of the `id` parameter.
        let id = url
            .split("id=")
            .nth(1)
            .and_then(|rest| rest.split('&').next())
            .expect("id parameter");
        assert!(!id.contains('&'), "unencoded '&' in the id: {}", url);
        assert!(!id.contains('='), "unencoded '=' in the id: {}", url);
        assert!(!id.contains('/'), "unencoded '/' in the id: {}", url);
        assert!(id.contains("%2F") && id.contains("%26") && id.contains("%3D"), "{}", url);
    }

    #[test]
    fn error_codes_are_translated() {
        assert_eq!(describe_error_code(40, ""), "wrong username or password/token");
        assert_eq!(describe_error_code(30, "bad version"),
                   "server rejected the requested API version: bad version");
        assert_eq!(describe_error_code(0, ""), "unspecified error");
        assert_eq!(describe_error_code(0, "weird"), "weird");
    }

    #[test]
    fn missing_status_field_is_tolerated() {
        // A slightly off-spec server that omits `status` must keep working.
        let sub = serde_json::json!({ "artists": {} });
        assert!(check_subsonic_status(&sub, "getArtists.view").is_ok());
    }
}
