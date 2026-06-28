use anyhow::Result;
use crate::config::Config;
use crate::api::{build_auth_query, models::*};

pub async fn get_artists(config: &Config) -> Result<Vec<Artist>> {
    let client   = reqwest::Client::new();
    let params   = build_auth_query(config);
    let response = client
        .get(format!("{}/rest/getArtists", config.server.url))
        .query(&params).send().await?;
    let body: SubsonicResponse = serde_json::from_str(&response.text().await?)?;
    match body.response.content {
        ContentType::Artists { artists } =>
            Ok(artists.index.into_iter().flat_map(|g| g.artist).collect()),
        _ => anyhow::bail!("Unexpected response for artists"),
    }
}

pub async fn get_artist_albums(artist_id: &str, config: &Config) -> Result<Vec<Album>> {
    let client     = reqwest::Client::new();
    let mut params = build_auth_query(config);
    params.push(("id".to_string(), artist_id.to_string()));
    let response = client
        .get(format!("{}/rest/getArtist", config.server.url))
        .query(&params).send().await?;
    let body: SubsonicResponse = serde_json::from_str(&response.text().await?)?;
    match body.response.content {
        ContentType::Albums { artist } => Ok(artist.album),
        _ => anyhow::bail!("Unexpected response for albums"),
    }
}

pub async fn get_album_songs(album_id: &str, config: &Config) -> Result<Vec<Song>> {
    let client     = reqwest::Client::new();
    let mut params = build_auth_query(config);
    params.push(("id".to_string(), album_id.to_string()));
    let response = client
        .get(format!("{}/rest/getAlbum", config.server.url))
        .query(&params).send().await?;
    let body: SubsonicResponse = serde_json::from_str(&response.text().await?)?;
    match body.response.content {
        ContentType::Songs { album } => Ok(album.song),
        _ => anyhow::bail!("Unexpected response for songs"),
    }
}

pub async fn get_playlists(config: &Config) -> Result<Vec<Playlist>> {
    let client   = reqwest::Client::new();
    let params   = build_auth_query(config);
    let response = client
        .get(format!("{}/rest/getPlaylists", config.server.url))
        .query(&params).send().await?;
    let body: SubsonicResponse = serde_json::from_str(&response.text().await?)?;
    match body.response.content {
        ContentType::Playlists { playlists } => Ok(playlists.playlist),
        _ => anyhow::bail!("Unexpected response for playlists"),
    }
}

pub async fn get_playlist_songs(playlist_id: &str, config: &Config) -> Result<Vec<Song>> {
    let client     = reqwest::Client::new();
    let mut params = build_auth_query(config);
    params.push(("id".to_string(), playlist_id.to_string()));
    let response = client
        .get(format!("{}/rest/getPlaylist", config.server.url))
        .query(&params).send().await?;
    let body: SubsonicResponse = serde_json::from_str(&response.text().await?)?;
    match body.response.content {
        ContentType::PlaylistDetail { playlist } => Ok(playlist.entry),
        _ => anyhow::bail!("Unexpected response for playlist songs"),
    }
}

pub async fn get_random_songs(config: &Config, count: u16) -> Result<Vec<Song>> {
    let client     = reqwest::Client::new();
    let mut params = build_auth_query(config);
    params.push(("size".to_string(), count.to_string()));
    let response = client
        .get(format!("{}/rest/getRandomSongs", config.server.url))
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

pub async fn search_songs(query: &str, config: &Config) -> Result<Vec<Song>> {
    let client     = reqwest::Client::new();
    let mut params = build_auth_query(config);
    params.push(("query".to_string(),     query.to_string()));
    params.push(("songCount".to_string(), "100".to_string()));
    let response = client
        .get(format!("{}/rest/search3", config.server.url))
        .query(&params).send().await?;
    let body = response.text().await?;
    let parsed: SubsonicResponse = match serde_json::from_str(&body) {
        Ok(p)  => p,
        Err(e) => {
            eprintln!("JSON Parse Error: {}", e);
            anyhow::bail!("Failed to parse search response");
        }
    };
    match parsed.response.content {
        ContentType::SearchResults { search_result3 } => Ok(search_result3.song),
        other => {
            eprintln!("Unexpected search response: {:#?}", other);
            Ok(Vec::new())
        }
    }
}

pub async fn scrobble(song_id: &str, timestamp_ms: u128, config: &Config) -> Result<()> {
    let client     = reqwest::Client::new();
    let mut params = build_auth_query(config);
    params.push(("id".to_string(),         song_id.to_string()));
    params.push(("time".to_string(),        timestamp_ms.to_string()));
    params.push(("submission".to_string(),  "true".to_string()));
    let response = client
        .get(format!("{}/rest/scrobble", config.server.url))
        .query(&params).send().await?;
    if !response.status().is_success() {
        eprintln!("Scrobble failed: {}", response.text().await.unwrap_or_default());
    }
    Ok(())
}
