use reqwest::Client;
use serde::Deserialize;
use std::collections::HashMap;

#[derive(Debug, Deserialize)]
pub struct Artist {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Deserialize)]
pub struct Album {
    pub id: String,
    pub name: String,
    pub artist: String,
}

#[derive(Debug, Deserialize)]
pub struct Song {
    pub id: String,
    pub title: String,
    pub artist: String,
    pub duration: u32,
}

#[derive(Clone)]
pub struct NavidromeClient {
    client: Client,
    base_url: String,
    auth_params: HashMap<String, String>,
}

impl NavidromeClient {
    pub fn new(config: &crate::config::AppConfig) -> Result<Self, anyhow::Error> {
        let client = Client::new();
        
        let token = format!("{:x}", md5::compute(format!(
            "{}:{}", 
            config.server.username, 
            config.server.password
        )));

        let mut auth_params = HashMap::new();
        auth_params.insert("u".into(), config.server.username.clone());
        auth_params.insert("t".into(), token);
        auth_params.insert("s".into(), "termnavi".into());
        auth_params.insert("v".into(), "1.16.1".into());
        auth_params.insert("c".into(), "termnavi".into());
        auth_params.insert("f".into(), "json".into());

        Ok(Self {
            client,
            base_url: config.server.url.clone(),
            auth_params,
        })
    }

    pub async fn get_artists(&self) -> Result<Vec<Artist>, anyhow::Error> {
        let mut url = format!("{}/rest/getArtists", self.base_url);
        self.add_auth_params(&mut url);
        
        let response = self.client.get(&url).send().await?;
        Ok(response.json().await?)
    }

    fn add_auth_params(&self, url: &mut String) {
        let params = self.auth_params.iter()
            .map(|(k, v)| format!("{}={}", k, v))
            .collect::<Vec<_>>()
            .join("&");
        
        if !params.is_empty() {
            url.push('?');
            url.push_str(&params);
        }
    }
}