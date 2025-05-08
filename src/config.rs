use serde::Deserialize;
use std::fs;

#[derive(Debug, Deserialize, Clone)]
pub struct PlayerConfig {
    pub use_mpv: bool,
    pub experimental_audio: bool,
}

#[derive(Debug, Deserialize, Clone)]
pub struct AppConfig {
    pub server_url: String,
    pub username: String,
    pub password: String,
    pub player: PlayerConfig,
}

impl AppConfig {
    pub fn load() -> Result<Self, Box<dyn std::error::Error>> {
        let config_data = fs::read_to_string("config.toml")?;
        let config: AppConfig = toml::from_str(&config_data)?;
        Ok(config)
    }
}
