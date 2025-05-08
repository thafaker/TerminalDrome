use crate::config::AppConfig;
use anyhow::Result;
use md5;
use rand::{distributions::Alphanumeric, Rng};
use serde::Deserialize;
use std::collections::HashMap;

#[derive(Debug, Deserialize)]
pub struct SubsonicResponse<T> {
    pub subsonic_response: T,
}

#[derive(Debug, Deserialize)]
pub struct ArtistListResponse {
    pub artists: ArtistIndex,
}

#[derive(Debug, Deserialize)]
pub struct ArtistIndex {
    pub index: Vec<ArtistGroup>,
}

#[derive(Debug, Deserialize)]
pub struct ArtistGroup {
    pub name: String,
    pub artist: Vec<Artist>,
}

#[derive(Debug, Deserialize)]
pub struct Artist {
    pub id: String,
    pub name: String,
}

pub fn get_artists(config: &AppConfig) -> Result<Vec<Artist>> {
    let salt: String = rand::thread_rng()
        .sample_iter(&Alphanumeric)
        .take(6)
        .map(char::from)
        .collect();

    let token = format!("{:x}", md5::compute(format!("{}{}", config.password, salt)));

    let mut params = HashMap::new();
    params.insert("u", &config.username);
    params.insert("t", &token);
    params.insert("s", &salt);
    params.insert("v", "1.16.1");
    params.insert("c", "termnavi");
    params.insert("f", "json");

    let url = format!("{}/rest/getArtists", config.server_url);

    let client = reqwest::blocking::Client::new();
    let response = client.get(&url).query(&params).send()?;

    if !response.status().is_success() {
        anyhow::bail!("HTTP-Fehler: {}", response.status());
    }

    let parsed: SubsonicResponse<ArtistListResponse> = response.json()?;
    let mut artists = Vec::new();

    for group in parsed.subsonic_response.artists.index {
        artists.extend(group.artist);
    }

    Ok(artists)
}
