use crate::api::models::{Album, Artist, Playlist, Song, SongDetail};
use crate::config::{Config, ServerConfig};
use anyhow::{bail, Result};
use reqwest::Client;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MusicSource {
    Navidrome,
    Bandcamp,
}

pub fn get_target_config(source: MusicSource, config: &Config) -> &ServerConfig {
    match source {
        MusicSource::Navidrome => &config.server,
        MusicSource::Bandcamp => config.bandcamp.as_ref().unwrap_or(&config.server),
    }
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

    if let (Some(token), Some(salt)) = (&target.token, &target.salt) {
        params.push(("t", token.clone()));
        params.push(("s", salt.clone()));
    } else if let Some(pass) = &target.password {
        let salt = "c5a6b7";
        let digest = md5::compute(format!("{}{}", pass, salt));
        let token = format!("{:x}", digest);
        params.push(("t", token));
        params.push(("s", salt.to_string()));
    }

    params
}

pub async fn get_artists(source: MusicSource, config: &Config) -> Result<Vec<Artist>> {
    let client = Client::new();
    let target = get_target_config(source, config);
    let params = build_auth_query_for_source(source, config);
    let url = format!("{}/rest/getArtists.view", target.url.trim_end_matches('/'));

    let response = client.get(&url).query(&params).send().await?;
    let body: serde_json::Value = response.json().await?;

    let mut artists = Vec::new();
    if let Some(index_list) = body["subsonic-response"]["artists"]["index"].as_array() {
        for idx in index_list {
            if let Some(artist_list) = idx["artist"].as_array() {
                for a in artist_list {
                    if let (Some(id), Some(name)) = (a["id"].as_str(), a["name"].as_str()) {
                        artists.push(Artist {
                            id: id.to_string(),
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
    let client = Client::new();
    let target = get_target_config(source, config);
    let params = build_auth_query_for_source(source, config);
    let url = format!("{}/rest/getPlaylists.view", target.url.trim_end_matches('/'));

    let response = client.get(&url).query(&params).send().await?;
    let body: serde_json::Value = response.json().await?;

    let mut playlists = Vec::new();
    if let Some(list) = body["subsonic-response"]["playlists"]["playlist"].as_array() {
        for p in list {
            if let (Some(id), Some(name)) = (p["id"].as_str(), p["name"].as_str()) {
                playlists.push(Playlist {
                    id: id.to_string(),
                    name: name.to_string(),
                    comment: p["comment"].as_str().map(|s| s.to_string()),
                    song_count: p["songCount"].as_u64().unwrap_or(0) as u32,
                    duration: p["duration"].as_u64().unwrap_or(0),
                    cover_art: p["coverArt"].as_str().map(|s| s.to_string()),
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
    let client = Client::new();
    let target = get_target_config(source, config);
    let mut params = build_auth_query_for_source(source, config);
    params.push(("id", artist_id.to_string()));
    let url = format!("{}/rest/getArtist.view", target.url.trim_end_matches('/'));

    let response = client.get(&url).query(&params).send().await?;
    let body: serde_json::Value = response.json().await?;

    let mut albums = Vec::new();
    if let Some(list) = body["subsonic-response"]["artist"]["album"].as_array() {
        for a in list {
            if let (Some(id), Some(name)) = (a["id"].as_str().or_else(|| a["title"].as_str()), a["name"].as_str().or_else(|| a["title"].as_str())) {
                albums.push(Album {
                    id: id.to_string(),
                    name: name.to_string(),
                    artist: a["artist"].as_str().unwrap_or("Unknown").to_string(),
                    year: a["year"].as_i64().map(|y| y as i32),
                    song_count: a["songCount"].as_u64().unwrap_or(0) as u32,
                    cover_art: a["coverArt"].as_str().map(|s| s.to_string()),
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
    let client = Client::new();
    let target = get_target_config(source, config);
    let mut params = build_auth_query_for_source(source, config);
    params.push(("id", album_id.to_string()));
    let url = format!("{}/rest/getAlbum.view", target.url.trim_end_matches('/'));

    let response = client.get(&url).query(&params).send().await?;
    let body: serde_json::Value = response.json().await?;

    let mut songs = Vec::new();
    if let Some(list) = body["subsonic-response"]["album"]["song"].as_array() {
        for s in list {
            if let (Some(id), Some(title)) = (s["id"].as_str(), s["title"].as_str()) {
                songs.push(Song {
                    id: id.to_string(),
                    title: title.to_string(),
                    artist: s["artist"].as_str().map(|s| s.to_string()),
                    album: s["album"].as_str().map(|s| s.to_string()),
                    duration: s["duration"].as_u64().unwrap_or(0),
                    track: s["track"].as_u64().map(|t| t as u32),
                    starred: if s["starred"].is_string() {
                        s["starred"].as_str().map(|str_val| str_val.to_string())
                    } else {
                        None
                    },
                });
            }
        }
    }
    Ok(songs)
}

pub async fn get_playlist_songs(
    source: MusicSource,
    playlist_id: &str,
    config: &Config,
) -> Result<Vec<Song>> {
    let client = Client::new();
    let target = get_target_config(source, config);
    let mut params = build_auth_query_for_source(source, config);
    params.push(("id", playlist_id.to_string()));
    let url = format!("{}/rest/getPlaylist.view", target.url.trim_end_matches('/'));

    let response = client.get(&url).query(&params).send().await?;
    let body: serde_json::Value = response.json().await?;

    let mut songs = Vec::new();
    if let Some(list) = body["subsonic-response"]["playlist"]["entry"].as_array() {
        for s in list {
            if let (Some(id), Some(title)) = (s["id"].as_str(), s["title"].as_str()) {
                songs.push(Song {
                    id: id.to_string(),
                    title: title.to_string(),
                    artist: s["artist"].as_str().map(|s| s.to_string()),
                    album: s["album"].as_str().map(|s| s.to_string()),
                    duration: s["duration"].as_u64().unwrap_or(0),
                    track: s["track"].as_u64().map(|t| t as u32),
                    starred: if s["starred"].is_string() {
                        s["starred"].as_str().map(|str_val| str_val.to_string())
                    } else {
                        None
                    },
                });
            }
        }
    }
    Ok(songs)
}

pub async fn get_random_songs(
    source: MusicSource,
    config: &Config,
    size: usize,
) -> Result<Vec<Song>> {
    let client = Client::new();
    let target = get_target_config(source, config);
    let mut params = build_auth_query_for_source(source, config);
    params.push(("size", size.to_string()));
    let url = format!("{}/rest/getRandomSongs.view", target.url.trim_end_matches('/'));

    let response = client.get(&url).query(&params).send().await?;
    let body: serde_json::Value = response.json().await?;

    let mut songs = Vec::new();
    if let Some(list) = body["subsonic-response"]["randomSongs"]["song"].as_array() {
        for s in list {
            if let (Some(id), Some(title)) = (s["id"].as_str(), s["title"].as_str()) {
                songs.push(Song {
                    id: id.to_string(),
                    title: title.to_string(),
                    artist: s["artist"].as_str().map(|s| s.to_string()),
                    album: s["album"].as_str().map(|s| s.to_string()),
                    duration: s["duration"].as_u64().unwrap_or(0),
                    track: s["track"].as_u64().map(|t| t as u32),
                    starred: if s["starred"].is_string() {
                        s["starred"].as_str().map(|str_val| str_val.to_string())
                    } else {
                        None
                    },
                });
            }
        }
    }
    Ok(songs)
}

pub async fn search_songs(
    source: MusicSource,
    query: &str,
    config: &Config,
) -> Result<Vec<Song>> {
    let client = Client::new();
    let target = get_target_config(source, config);
    let mut params = build_auth_query_for_source(source, config);
    params.push(("query", query.to_string()));
    params.push(("songCount", "50".to_string()));
    let url = format!("{}/rest/search3.view", target.url.trim_end_matches('/'));

    let response = client.get(&url).query(&params).send().await?;
    let body: serde_json::Value = response.json().await?;

    let mut songs = Vec::new();
    if let Some(list) = body["subsonic-response"]["searchResult3"]["song"].as_array() {
        for s in list {
            if let (Some(id), Some(title)) = (s["id"].as_str(), s["title"].as_str()) {
                songs.push(Song {
                    id: id.to_string(),
                    title: title.to_string(),
                    artist: s["artist"].as_str().map(|s| s.to_string()),
                    album: s["album"].as_str().map(|s| s.to_string()),
                    duration: s["duration"].as_u64().unwrap_or(0),
                    track: s["track"].as_u64().map(|t| t as u32),
                    starred: if s["starred"].is_string() {
                        s["starred"].as_str().map(|str_val| str_val.to_string())
                    } else {
                        None
                    },
                });
            }
        }
    }
    Ok(songs)
}

pub async fn star_song(source: MusicSource, song_id: &str, config: &Config) -> Result<()> {
    let client = Client::new();
    let target = get_target_config(source, config);
    let mut params = build_auth_query_for_source(source, config);
    params.push(("id", song_id.to_string()));
    let url = format!("{}/rest/star.view", target.url.trim_end_matches('/'));

    let response = client.get(&url).query(&params).send().await?;
    if !response.status().is_success() {
        bail!("Failed to star song");
    }
    Ok(())
}

pub async fn scrobble(
    source: MusicSource,
    song_id: &str,
    timestamp_ms: u128,
    config: &Config,
) -> Result<()> {
    let client = Client::new();
    let target = get_target_config(source, config);
    let mut params = build_auth_query_for_source(source, config);
    params.push(("id", song_id.to_string()));
    params.push(("time", timestamp_ms.to_string()));
    params.push(("submission", "true".to_string()));
    let url = format!("{}/rest/scrobble.view", target.url.trim_end_matches('/'));

    let response = client.get(&url).query(&params).send().await?;
    if !response.status().is_success() {
        bail!("Failed to scrobble song");
    }
    Ok(())
}

pub async fn get_song_info(
    source: MusicSource,
    song_id: &str,
    config: &Config,
) -> Result<SongDetail> {
    let client = Client::new();
    let target = get_target_config(source, config);
    let mut params = build_auth_query_for_source(source, config);
    params.push(("id", song_id.to_string()));
    let url = format!("{}/rest/getSong.view", target.url.trim_end_matches('/'));

    let response = client.get(&url).query(&params).send().await?;
    let body: serde_json::Value = response.json().await?;

    let song_json = &body["subsonic-response"]["song"];
    if song_json.is_null() {
        bail!("Server returned no song data");
    }
    let detail: SongDetail = serde_json::from_value(song_json.clone())?;
    Ok(detail)
}

// ── Playlist management ──────────────────────────────────────────────────────

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
    let client = Client::new();
    let target = get_target_config(source, config);
    let mut params = build_auth_query_for_source(source, config);
    params.push(("name", name.to_string()));
    if let Some(song_id) = initial_song_id {
        params.push(("songId", song_id.to_string()));
    }
    let url = format!("{}/rest/createPlaylist.view", target.url.trim_end_matches('/'));

    let response = client.get(&url).query(&params).send().await?;
    let status = response.status();
    let body: serde_json::Value = response.json().await?;

    if !status.is_success() {
        bail!("createPlaylist HTTP {}", status);
    }
    let sub = &body["subsonic-response"];
    if sub["status"].as_str() == Some("failed") {
        let msg = sub["error"]["message"].as_str().unwrap_or("unknown error");
        bail!("createPlaylist failed: {}", msg);
    }

    // The response may include the newly created playlist id under
    // `playlist.id`. Some servers omit it, so we return an empty string
    // in that case — the caller can refetch the playlist list.
    Ok(sub["playlist"]["id"].as_str().unwrap_or("").to_string())
}

/// Add a song to an existing playlist. Returns Ok(()) even if the server
/// silently ignores the request (some Bandcamp bridges are lenient here).
pub async fn add_song_to_playlist(
    source: MusicSource,
    playlist_id: &str,
    song_id: &str,
    config: &Config,
) -> Result<()> {
    let client = Client::new();
    let target = get_target_config(source, config);
    let mut params = build_auth_query_for_source(source, config);
    params.push(("playlistId", playlist_id.to_string()));
    params.push(("songIdToAdd", song_id.to_string()));
    let url = format!("{}/rest/updatePlaylist.view", target.url.trim_end_matches('/'));

    let response = client.get(&url).query(&params).send().await?;
    let status = response.status();
    let body: serde_json::Value = response.json().await?;

    if !status.is_success() {
        bail!("updatePlaylist HTTP {}", status);
    }
    let sub = &body["subsonic-response"];
    if sub["status"].as_str() == Some("failed") {
        let msg = sub["error"]["message"].as_str().unwrap_or("unknown error");
        bail!("add to playlist failed: {}", msg);
    }
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
    let client = Client::new();
    let target = get_target_config(source, config);
    let mut params = build_auth_query_for_source(source, config);
    params.push(("playlistId", playlist_id.to_string()));
    params.push(("songIndexToRemove", song_index.to_string()));
    let url = format!("{}/rest/updatePlaylist.view", target.url.trim_end_matches('/'));

    let response = client.get(&url).query(&params).send().await?;
    let status = response.status();
    let body: serde_json::Value = response.json().await?;

    if !status.is_success() {
        bail!("updatePlaylist HTTP {}", status);
    }
    let sub = &body["subsonic-response"];
    if sub["status"].as_str() == Some("failed") {
        let msg = sub["error"]["message"].as_str().unwrap_or("unknown error");
        bail!("remove from playlist failed: {}", msg);
    }
    Ok(())
}

/// Delete a whole playlist.
#[allow(dead_code)]
pub async fn delete_playlist(
    source: MusicSource,
    playlist_id: &str,
    config: &Config,
) -> Result<()> {
    let client = Client::new();
    let target = get_target_config(source, config);
    let mut params = build_auth_query_for_source(source, config);
    params.push(("id", playlist_id.to_string()));
    let url = format!("{}/rest/deletePlaylist.view", target.url.trim_end_matches('/'));

    let response = client.get(&url).query(&params).send().await?;
    if !response.status().is_success() {
        bail!("deletePlaylist HTTP {}", response.status());
    }
    Ok(())
}