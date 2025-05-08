use reqwest::Error;
use serde::Deserialize;
use std::collections::HashMap;
use md5;

#[derive(Deserialize, Debug)]
pub struct Artist {
    pub id: String,
    pub name: String,
}

#[derive(Deserialize, Debug)]
pub struct Song {
    pub id: String,
    pub title: String,
    pub artist: String,
    #[allow(dead_code)]
    pub duration: u32,
}

pub fn get_artists(config: &crate::config::AppConfig) -> Result<Vec<Artist>, Error> {
    let client = Client::new();
    let params = auth_params(config);
    
    client.get(&format!("{}/rest/getArtists", config.server.url))
        .query(&params)
        .send()?
        .json::<Vec<Artist>>()
}

pub fn get_songs_by_artist(config: &crate::config::AppConfig, artist_id: &str) -> Result<Vec<Song>, Error> {
    let client = Client::new();
    let mut params = auth_params(config);
    params.insert("artistId".to_string(), artist_id.to_string());
    
    client.get(&format!("{}/rest/getSongs", config.server.url))
        .query(&params)
        .send()?
        .json::<Vec<Song>>()
}

fn auth_params(config: &crate::config::AppConfig) -> HashMap<String, String> {
    let token = format!("{:x}", md5::compute(format!(
        "{}:{}", 
        config.server.username, 
        config.server.password
    )));
    
    let mut params = HashMap::new();
    params.insert("u".to_string(), config.server.username.clone());
    params.insert("t".to_string(), token);
    params.insert("s".to_string(), "termnavi".to_string());
    params.insert("v".to_string(), "1.16.1".to_string());
    params.insert("c".to_string(), "termnavi".to_string());
    params.insert("f".to_string(), "json".to_string());
    params
}
