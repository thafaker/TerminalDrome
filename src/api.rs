use reqwest::blocking::Client;
use reqwest::Error;
use serde::Deserialize;
use std::collections::HashMap;

#[derive(Deserialize, Debug)]
pub struct Artist {
    pub name: String,
}

pub fn get_artists(config: &crate::config::AppConfig) -> Result<Vec<Artist>, Error> {
    let client = Client::new();
    
    let mut params: HashMap<&str, &str> = HashMap::new();
    params.insert("u", &config.username[..]);
    params.insert("p", &config.password[..]);
    params.insert("v", "1.16.1");
    params.insert("c", "termnavi");
    params.insert("f", "json");

    let res = client
        .get(&format!("{}/rest/artist.list", config.server_url))
        .query(&params)
        .send()?
        .json::<Vec<Artist>>()?;

    Ok(res)
}
