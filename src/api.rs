use anyhow::{Context, Result};
use quick_xml::de::from_str;
use serde::Deserialize;
use std::time::Duration;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case")]
struct SubsonicResponse {
    #[serde(rename = "@status")]
    status: String,
    artists: Artists,
}

#[derive(Debug, Deserialize)]
struct Artists {
    #[serde(rename = "index", default)]
    indexes: Vec<Index>,
}

#[derive(Debug, Deserialize)]
struct Index {
    #[serde(rename = "artist", default)]
    artists: Vec<Artist>,
}

#[derive(Debug, Deserialize)]
struct Artist {
    name: String,
    id: String,
}

pub struct NavidromeClient {
    server_url: String,
    auth: (String, String),
}

impl NavidromeClient {
    pub fn get_artists(&self) -> Result<Vec<String>> {
        let url = format!(
            "{}/rest/getArtists?u={}&p={}&v=1.16.1&c=termnavi-0.1.0&f=xml",
            self.server_url, self.auth.0, self.auth.1
        );

        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(15))
            .danger_accept_invalid_certs(true)
            .build()?;

        let resp = client.get(&url).send()?;
        let xml_data = resp.text()?;

        let response: SubsonicResponse = from_str(&xml_data)?;
        
        if response.status != "ok" {
            anyhow::bail!("API returned non-ok status: {}", response.status);
        }

        let artists = response.artists
            .indexes
            .into_iter()
            .flat_map(|index| index.artists)
            .map(|artist| artist.name)
            .collect();

        Ok(artists)
    }

    pub fn get_stream_url(&self, id: &str) -> String {
        format!(
            "{}/rest/stream?id={}&u={}&p={}&c=termnavi-0.1.0",
            self.server_url, id, self.auth.0, self.auth.1
        )
    }
}