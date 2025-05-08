use crate::config::Config;
use md5;
use rand::{distributions::Alphanumeric, Rng};
use reqwest::blocking::Client;
use serde::Deserialize;
use std::error::Error;

#[derive(Debug, Deserialize)]
struct SubsonicResponse {
    #[serde(rename = "subsonic-response")]
    pub subsonic_response: SubsonicArtistsWrapper,
}

#[derive(Debug, Deserialize)]
pub struct SubsonicArtistsWrapper {
    pub status: String,
    pub artists: Option<Artists>,
}

#[derive(Debug, Deserialize)]
pub struct Artists {
    pub index: Vec<ArtistIndex>,
}

#[derive(Debug, Deserialize)]
pub struct ArtistIndex {
    pub name: String,
    pub artist: Vec<Artist>,
}

#[derive(Debug, Deserialize)]
pub struct Artist {
    pub id: String,
    pub name: String,
}

fn generate_salt() -> String {
    rand::thread_rng()
        .sample_iter(&Alphanumeric)
        .take(10)
        .map(char::from)
        .collect()
}

pub fn get_artists(config: &Config) -> Result<Vec<Artist>, Box<dyn Error>> {
    let salt = generate_salt();
    let token = format!("{:x}", md5::compute(format!("{}{}", config.password, salt)));

    let url = format!(
        "{}/rest/getArtists.view?u={}&t={}&s={}&v=1.16.1&c=termnavi&f=json",
        config.server_url, config.username, token, salt
    );

    let client = Client::new();
    let response = client.get(&url).send()?;

    if !response.status().is_success() {
        return Err(format!("Server returned HTTP {}", response.status()).into());
    }

    let subsonic: SubsonicResponse = response.json()?;

    if subsonic.subsonic_response.status != "ok" {
        return Err("Subsonic API returned error status".into());
    }

    let mut result = Vec::new();
    if let Some(artists) = subsonic.subsonic_response.artists {
        for index in artists.index {
            for artist in index.artist {
                result.push(artist);
            }
        }
    }

    Ok(result)
}
