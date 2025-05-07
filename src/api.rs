use anyhow::{Context, Result};
use quick_xml::de::from_str;
use serde::Deserialize;
use std::time::Duration;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case")]
struct SubsonicResponse {
    #[serde(rename = "@status")]
    status: String,
    #[serde(default)]
    artists: Option<Artists>,
}

#[derive(Debug, Deserialize, Default)]
struct Artists {
    #[serde(rename = "index", default)]
    indexes: Vec<Index>,
    #[serde(rename = "artist", default)]
    direct_artists: Vec<Artist>,
}

#[derive(Debug, Deserialize)]
struct Index {
    #[serde(rename = "artist", default)]
    artists: Vec<Artist>,
}

#[derive(Debug, Deserialize)]
struct Artist {
    #[serde(default)]
    name: String,
    #[serde(default)]
    id: String,
}

impl NavidromeClient {
    pub fn get_artists(&self) -> Result<Vec<(String, String)>> {
        let url = format!(
            "{}/rest/getArtists?u={}&p={}&v=1.16.1&c=termnavi-0.1.0&f=xml",
            self.server_url, self.auth.0, self.auth.1
        );

        let resp = reqwest::blocking::get(&url)?;
        let xml_data = resp.text()?;
        println!("Raw XML:\n{}", xml_data); // Debug-Ausgabe

        let response: SubsonicResponse = from_str(&xml_data)?;
        
        if response.status != "ok" {
            anyhow::bail!("API error: {}", response.status);
        }

        let artists = match response.artists {
            Some(Artists { indexes, direct_artists }) => {
                let from_indexes = indexes.into_iter().flat_map(|i| i.artists);
                from_indexes.chain(direct_artists.into_iter()).collect()
            }
            None => Vec::new(),
        };

        Ok(artists.into_iter()
           .filter(|a| !a.name.is_empty())
           .map(|a| (a.name, a.id))
           .collect())
    }
}