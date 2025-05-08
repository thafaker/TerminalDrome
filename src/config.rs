use serde::Deserialize;
use std::{fs, error::Error};

#[derive(Debug, Deserialize, Clone)]
pub struct ServerConfig {
    pub url: String,
    pub username: String,
    pub password: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct PlayerConfig {
    pub use_mpv: bool,
}

#[derive(Debug, Deserialize, Clone)]
pub struct AppConfig {
    pub server: ServerConfig,
    pub player: PlayerConfig,
}

impl AppConfig {
    pub fn load() -> Result<Self, Box<dyn Error + Send + Sync + 'static>> {
        let config_data = fs::read_to_string("config.toml")?;
        let config: AppConfig = toml::from_str(&config_data)?;
        Ok(config)
    }
}