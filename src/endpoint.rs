// src/api/endpoints.rs
use super::NavidromeClient;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct Artist {
    pub id: String,
    pub name: String,
    pub album_count: i32,
}

#[derive(Debug, Deserialize)]
pub struct Album {
    pub id: String,
    pub name: String,
    pub artist: String,
    pub song_count: i32,
}

#[derive(Debug, Deserialize)]
pub struct Song {
    pub id: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub duration: u32,
    pub path: String,
}

impl NavidromeClient {
    pub async fn get_artists(&self) -> Result<Vec<Artist>, anyhow::Error> {
        #[derive(Deserialize)]
        struct Response {
            artists: Vec<Artist>,
        }
        
        let res: Response = self.get("rest/getArtists", None).await?;
        Ok(res.artists)
    }

    pub async fn get_albums(&self, artist_id: &str) -> Result<Vec<Album>, anyhow::Error> {
        #[derive(Deserialize)]
        struct Response {
            albums: Vec<Album>,
        }
        
        let mut params = HashMap::new();
        params.insert("artistId".into(), artist_id.into());
        
        let res: Response = self.get("rest/getAlbumList", Some(params)).await?;
        Ok(res.albums)
    }

    pub async fn get_songs(&self, album_id: &str) -> Result<Vec<Song>, anyhow::Error> {
        #[derive(Deserialize)]
        struct Response {
            songs: Vec<Song>,
        }
        
        let mut params = HashMap::new();
        params.insert("albumId".into(), album_id.into());
        
        let res: Response = self.get("rest/getSongList", Some(params)).await?;
        Ok(res.songs)
    }

    pub async fn get_play_url(&self, song_id: &str) -> String {
        format!("{}/rest/stream?id={}&{}", 
            self.base_url,
            song_id,
            self.auth_params.iter()
                .map(|(k, v)| format!("{}={}", k, v))
                .collect::<Vec<_>>()
                .join("&")
        )
    }
}