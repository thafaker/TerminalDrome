use serde::Deserialize;
use std::{fs, error::Error};

#[derive(Debug, Deserialize, Clone)]
pub struct PlayerConfig {
    pub use_mpv: bool,
    #[allow(dead_code)] // Wir ignorieren die Warnung für dieses Feld
    pub experimental_audio: bool,
}

#[derive(Debug, Deserialize, Clone)]
pub struct AppConfig {
    #[serde(rename = "server")] // Korrekte Zuordnung zum TOML-Abschnitt
    pub server: ServerConfig,
    pub player: PlayerConfig,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ServerConfig {
    pub url: String,
    pub username: String,
    pub password: String,
}

impl AppConfig {
    pub fn load() -> Result<Self, Box<dyn Error + Send + Sync + 'static>> {
        let config_path = "config.toml";
        let config_data = fs::read_to_string(config_path)
            .map_err(|e| format!("Could not read config file at '{}': {}", config_path, e))?;
        
        toml::from_str(&config_data)
            .map_err(|e| format!("TOML parse error: {}", e).into())
    }
}
