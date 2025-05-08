use reqwest::Client;
use serde::Deserialize;
use base64::Engine;

#[derive(Debug, Deserialize)]
pub struct Artist {
    pub id: String,
    pub name: String,
}

#[derive(Clone)]
pub struct NavidromeClient {
    client: Client,
    base_url: String,
    auth_header: String,
}

impl NavidromeClient {
    pub fn new(config: &crate::config::AppConfig) -> Result<Self, anyhow::Error> {
        let client = Client::new();
        
        // Basic Auth Header erstellen
        let auth = format!("{}:{}", config.server.username, config.server.password);
        let auth_header = format!(
            "Basic {}",
            base64::engine::general_purpose::STANDARD.encode(auth)
        );

        Ok(Self {
            client,
            base_url: config.server.url.clone(),
            auth_header,
        })
    }

    pub async fn get_artists(&self) -> Result<Vec<Artist>, anyhow::Error> {
        let url = format!("{}/rest/getArtists", self.base_url);
        
        let response = self.client
            .get(&url)
            .header("Authorization", &self.auth_header)
            .send()
            .await?;

        let artists = response.json().await?;
        Ok(artists)
    }
}