use anyhow::Result;
use quick_xml::de::from_str;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct SubsonicResponse {
    #[serde(rename = "@status")]
    pub status: String,
    #[serde(default)]
    pub artists: Option<Artists>,
}

#[derive(Debug, Deserialize, Default)]
pub struct Artists {
    #[serde(rename = "index", default)]
    pub indexes: Vec<Index>,
    #[serde(rename = "artist", default)]
    pub direct_artists: Vec<Artist>,
}

#[derive(Debug, Deserialize)]
pub struct Index {
    #[serde(rename = "artist", default)]
    pub artists: Vec<Artist>,
}

#[derive(Debug, Deserialize)]
pub struct Artist {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub id: String,
}

pub struct NavidromeClient {
    pub server_url: String,
    pub auth: (String, String),
}

impl NavidromeClient {
    pub fn new(server_url: String, username: String, password: String) -> Self {
        Self {
            server_url,
            auth: (username, password),
        }
    }

    pub fn get_artists(&self) -> Result<Vec<(String, String)>> {
        let url = format!(
            "{}/rest/getArtists?u={}&p={}&v=1.16.1&c=termnavi-0.1.0&f=xml",
            self.server_url, self.auth.0, self.auth.1
        );

        let resp = reqwest::blocking::get(&url)?;
        let xml_data = resp.text()?;
        println!("Raw XML:\n{}", xml_data);

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

    pub fn get_stream_url(&self, id: &str) -> String {
        format!(
            "{}/rest/stream?id={}&u={}&p={}&c=termnavi-0.1.0",
            self.server_url, id, self.auth.0, self.auth.1
        )
    }
}