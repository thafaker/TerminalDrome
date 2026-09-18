use rand::Rng;
use anyhow::Result;
use crate::config::{Config, ServerConfig};
use crate::api::models::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum MusicSource {
    Navidrome,
    Bandcamp,
}

/// Hilfsfunktion, um die richtige Server-Konfiguration basierend auf der Musikquelle zu ermitteln.
pub fn get_target_config<'a>(source: MusicSource, config: &'a Config) -> &'a ServerConfig {
    match source {
        MusicSource::Navidrome => &config.server,
        MusicSource::Bandcamp => config.bandcamp.as_ref().unwrap_or(&config.server),
    }
}

/// Generiert die Subsonic-Authentifizierungsparameter (u, p/t, s, v, c) für die spezifische Quelle.
pub fn build_auth_query_for_source(source: MusicSource, config: &Config) -> Vec<(String, String)> {
    let target = get_target_config(source, config);
    
    // Generiere dynamisches Token per Salt
    let salt: String = rand::thread_rng()
        .sample_iter(&rand::distributions::Alphanumeric)
        .take(8)
        .map(char::from)
        .collect();
    let token = format!("{:x}", md5::compute(format!("{}{}", target.password, salt)));

    let version = match source {
        MusicSource::Navidrome => "1.16.1".to_string(),
        MusicSource::Bandcamp  => "1.16.0".to_string(),
    };

    vec![
        ("u".to_string(), target.username.clone()),
        ("t".to_string(), token),
        ("s".to_string(), salt),
        ("v".to_string(), version),
        ("c".to_string(), "TerminalDrome".to_string()),
        ("f".to_string(), "json".to_string()),
    ]
}

pub async fn get_artists(source: MusicSource, config: &Config) -> Result<Vec<Artist>> {
    let client   = reqwest::Client::new();
    let target   = get_target_config(source, config);
    let params   = build_auth_query_for_source(source, config);
    
    let response = client
        .get(format!("{}/rest/getArtists", target.url))
        .query(&params).send().await?;
    let body: SubsonicResponse = serde_json::from_str(&response.text().await?)?;
    match body.response.content {
        ContentType::Artists { artists } =>
            Ok(artists.index.into_iter().flat_map(|g| g.artist).collect()),
        _ => anyhow::bail!("Unexpected response for artists"),
    }
}

pub async fn get_artist_albums(source: MusicSource, artist_id: &str, config: &Config) -> Result<Vec<Album>> {
    let client     = reqwest::Client::new();
    let target     = get_target_config(source, config);
    let mut params = build_auth_query_for_source(source, config);
    params.push(("id".to_string(), artist_id.to_string()));
    
    let response = client
        .get(format!("{}/rest/getArtist", target.url))
        .query(&params).send().await?;
    let body: SubsonicResponse = serde_json::from_str(&response.text().await?)?;
    match body.response.content {
        ContentType::Albums { artist } => Ok(artist.album),
        _ => anyhow::bail!("Unexpected response for albums"),
    }
}

pub async fn get_album_songs(source: MusicSource, album_id: &str, config: &Config) -> Result<Vec<Song>> {
    let client     = reqwest::Client::new();
    let target     = get_target_config(source, config);
    let mut params = build_auth_query_for_source(source, config);
    params.push(("id".to_string(), album_id.to_string()));
    
    let response = client
        .get(format!("{}/rest/getAlbum", target.url))
        .query(&params).send().await?;
    let body: SubsonicResponse = serde_json::from_str(&response.text().await?)?;
    match body.response.content {
        ContentType::Songs { album } => Ok(album.song),
        _ => anyhow::bail!("Unexpected response for songs"),
    }
}

pub async fn get_playlists(source: MusicSource, config: &Config) -> Result<Vec<Playlist>> {
    let client   = reqwest::Client::new();
    let target   = get_target_config(source, config);
    let params   = build_auth_query_for_source(source, config);
    
    let response = client
        .get(format!("{}/rest/getPlaylists", target.url))
        .query(&params).send().await?;
    let body: SubsonicResponse = serde_json::from_str(&response.text().await?)?;
    match body.response.content {
        ContentType::Playlists { playlists } => Ok(playlists.playlist),
        _ => anyhow::bail!("Unexpected response for playlists"),
    }
}

pub async fn get_playlist_songs(source: MusicSource, playlist_id: &str, config: &Config) -> Result<Vec<Song>> {
    let client     = reqwest::Client::new();
    let target     = get_target_config(source, config);
    let mut params = build_auth_query_for_source(source, config);
    params.push(("id".to_string(), playlist_id.to_string()));
    
    let response = client
        .get(format!("{}/rest/getPlaylist", target.url))
        .query(&params).send().await?;
    let body: SubsonicResponse = serde_json::from_str(&response.text().await?)?;
    match body.response.content {
        ContentType::PlaylistDetail { playlist } => Ok(playlist.entry),
        _ => anyhow::bail!("Unexpected response for playlist songs"),
    }
}

pub async fn get_random_songs(source: MusicSource, config: &Config, count: u16) -> Result<Vec<Song>> {
    let client     = reqwest::Client::new();
    let target     = get_target_config(source, config);
    let mut params = build_auth_query_for_source(source, config);
    params.push(("size".to_string(), count.to_string()));
    
    let response = client
        .get(format!("{}/rest/getRandomSongs", target.url))
        .query(&params).send().await?;
    let body: SubsonicResponse = match serde_json::from_str(&response.text().await?) {
        Ok(p)  => p,
        Err(e) => {
            eprintln!("getRandomSongs parse error: {}", e);
            anyhow::bail!("Failed to parse getRandomSongs");
        }
    };
    match body.response.content {
        ContentType::RandomSongs { random_songs } => Ok(random_songs.song),
        other => {
            eprintln!("Unexpected getRandomSongs response: {:#?}", other);
            Ok(Vec::new())
        }
    }
}

pub async fn search_songs(source: MusicSource, query: &str, config: &Config) -> Result<Vec<Song>> {
    let client     = reqwest::Client::new();
    let target     = get_target_config(source, config);
    let mut params = build_auth_query_for_source(source, config);
    params.push(("query".to_string(), query.to_string()));
    params.push(("songCount".to_string(), "100".to_string()));
    
    let response = client
        .get(format!("{}/rest/search3", target.url))
        .query(&params)
        .send()
        .await?;

    let body = response.text().await?;
    let json: serde_json::Value = match serde_json::from_str(&body) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("JSON Parse Error: {}", e);
            return Ok(Vec::new());
        }
    };

    if let Some(songs) = json
        .get("subsonic-response")
        .and_then(|r| r.get("searchResult3"))
        .and_then(|sr| sr.get("song"))
        .and_then(|s| s.as_array())
    {
        let mut result = Vec::new();
        for item in songs {
            if let Ok(song) = serde_json::from_value::<Song>(item.clone()) {
                result.push(song);
            } else {
                eprintln!("Failed to parse a song item: {:?}", item);
            }
        }
        Ok(result)
    } else {
        eprintln!("No songs found or unexpected response structure");
        Ok(Vec::new())
    }
}

pub async fn scrobble(source: MusicSource, song_id: &str, timestamp_ms: u128, config: &Config) -> Result<()> {
    let client     = reqwest::Client::new();
    let target     = get_target_config(source, config);
    let mut params = build_auth_query_for_source(source, config);
    params.push(("id".to_string(),         song_id.to_string()));
    params.push(("time".to_string(),        timestamp_ms.to_string()));
    params.push(("submission".to_string(),  "true".to_string()));
    
    let response = client
        .get(format!("{}/rest/scrobble", target.url))
        .query(&params).send().await?;
    if !response.status().is_success() {
        eprintln!("Scrobble failed: {}", response.text().await.unwrap_or_default());
    }
    Ok(())
}

pub async fn star_song(source: MusicSource, song_id: &str, config: &Config) -> Result<()> {
    let client     = reqwest::Client::new();
    let target     = get_target_config(source, config);
    let mut params = build_auth_query_for_source(source, config);
    params.push(("id".to_string(), song_id.to_string()));
    
    let response = client
        .get(format!("{}/rest/star", target.url))
        .query(&params)
        .send()
        .await?;
    
    if !response.status().is_success() {
        let error_text = response.text().await.unwrap_or_default();
        eprintln!("Star song failed: {}", error_text);
        anyhow::bail!("Failed to star song: {}", error_text);
    }
    
    Ok(())
}