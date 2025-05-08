use md5;
use rand::{distributions::Alphanumeric, Rng};
use reqwest::blocking::Client;
use serde::Deserialize;
use std::error::Error;
use std::fs;

#[derive(Debug)]
struct Config {
    server_url: String,
    username: String,
    password: String,
}

fn load_config() -> Config {
    // Beispiel: Konfiguration aus Datei "config.toml" einlesen (du kannst das anpassen)
    // Hier für einfaches Testing fest kodiert:
    Config {
        server_url: "https://music.apfelhammer.de".to_string(),
        username: "thahipster".to_string(),
        password: "t3st.k0tzE".to_string(),
    }
}

#[derive(Debug, Deserialize)]
struct SubsonicResponse {
    #[serde(rename = "subsonic-response")]
    pub subsonic_response: SubsonicArtistsWrapper,
}

#[derive(Debug, Deserialize)]
struct SubsonicArtistsWrapper {
    pub status: String,
    pub artists: Option<Artists>,
}

#[derive(Debug, Deserialize)]
struct Artists {
    pub index: Vec<ArtistIndex>,
}

#[derive(Debug, Deserialize)]
struct ArtistIndex {
    pub name: String,
    pub artist: Vec<Artist>,
}

#[derive(Debug, Deserialize)]
struct Artist {
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

fn get_artists(config: &Config) -> Result<Vec<Artist>, Box<dyn Error>> {
    let salt = generate_salt();
    let token = format!("{:x}", md5::compute(format!("{}{}", config.password, salt)));

    let url = format!(
        "{}/rest/getArtists.view?u={}&t={}&s={}&v=1.16.1&c=termnavi&f=json",
        config.server_url, config.username, token, salt
    );

    let client = Client::new();
    let response = client.get(&url).send()?;

    if !response.status().is_success() {
        return Err(format!("HTTP-Fehler: {}", response.status()).into());
    }

    let subsonic: SubsonicResponse = response.json()?;

    if subsonic.subsonic_response.status != "ok" {
        return Err("Subsonic-Status ist nicht 'ok'".into());
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

fn main() {
    let config = load_config();

    println!("Verbinde mit Server {}...", config.server_url);
    match get_artists(&config) {
        Ok(artists) => {
            println!("{} Künstler geladen:", artists.len());
            for artist in artists {
                println!("- [{}] {}", artist.id, artist.name);
            }
        }
        Err(e) => {
            eprintln!("Fehler beim Laden der Künstler: {}", e);
        }
    }
}
