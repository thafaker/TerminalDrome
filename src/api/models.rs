use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct SubsonicResponse {
    #[serde(rename = "subsonic-response")]
    pub response: SubsonicContent,
}

#[derive(Debug, Deserialize)]
pub struct SubsonicContent {
    #[serde(flatten)]
    pub content: ContentType,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
#[allow(dead_code)]
pub enum ContentType {
    Artists        { artists: ArtistList },
    Albums         { artist: ArtistDetail },
    Songs          { album: AlbumDetail },
    Directory      (MusicDirectory),
    SearchResults  { #[serde(rename = "searchResult3")] search_result3: SearchResult },
    Playlists      { playlists: PlaylistList },
    PlaylistDetail { playlist: PlaylistSongs },
    RandomSongs    { #[serde(rename = "randomSongs")] random_songs: RandomSongList },
}

#[derive(Debug, Deserialize)]
pub struct ArtistList {
    pub index: Vec<ArtistGroup>,
}

#[derive(Debug, Deserialize)]
pub struct ArtistGroup {
    #[serde(default)]
    pub artist: Vec<Artist>,
}

#[derive(Debug, Deserialize, Clone, Serialize)]
pub struct Artist {
    pub id:   String,
    pub name: String,
}

#[derive(Debug, Deserialize)]
pub struct ArtistDetail {
    pub album: Vec<Album>,
}

#[derive(Debug, Deserialize, Clone, Serialize)]
pub struct Album {
    pub id:        String,
    pub name:      String,
    pub artist:    String,
    #[serde(rename = "coverArt")]
    pub cover_art: Option<String>,
    pub year:      Option<i32>,
    #[serde(rename = "songCount")]
    pub song_count: u32,
}

#[derive(Debug, Deserialize)]
pub struct AlbumDetail {
    pub song: Vec<Song>,
}

#[derive(Debug, Deserialize, Clone, Serialize)]
pub struct Song {
    pub id:       String,
    pub title:    String,
    pub duration: u64,
    pub track:    Option<u32>,
    pub artist:   Option<String>,
    pub album:    Option<String>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct MusicDirectory {
    pub child: Vec<Song>,
}

#[derive(Debug, Deserialize)]
pub struct SearchResult {
    pub song: Vec<Song>,
}

#[derive(Debug, Deserialize)]
pub struct RandomSongList {
    #[serde(default)]
    pub song: Vec<Song>,
}

#[derive(Debug, Deserialize, Clone, Serialize)]
pub struct Playlist {
    pub id:         String,
    pub name:       String,
    #[serde(rename = "songCount")]
    pub song_count: u32,
    #[serde(default)]
    pub duration:   u64,
    #[serde(rename = "coverArt")]
    pub cover_art:  Option<String>,
    pub comment:    Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct PlaylistList {
    pub playlist: Vec<Playlist>,
}

#[derive(Debug, Deserialize)]
pub struct PlaylistSongs {
    #[serde(default)]
    pub entry: Vec<Song>,
}
