// src/config.rs
use serde::Deserialize;
use std::path::PathBuf;

#[derive(Debug, Deserialize)]
pub struct ServerConfig {
    pub url: String,
    pub username: String,
    pub password: String,
}

#[derive(Debug, Deserialize)]
pub struct PlayerConfig {
    #[allow(dead_code)]  // Wird später verwendet
    pub use_mpv: bool,
}

#[derive(Debug, Deserialize)]
pub struct AppConfig {
    pub server: ServerConfig,
    #[allow(dead_code)]  // Wird später verwendet
    pub player: PlayerConfig,
}

impl AppConfig {
    pub fn load() -> anyhow::Result<Self> {
        let config_path = Self::get_config_path()?;
        let config_content = std::fs::read_to_string(config_path)?;
        Ok(toml::from_str(&config_content)?)
    }

    fn get_config_path() -> anyhow::Result<PathBuf> {
        let config_dir = directories::ProjectDirs::from("org", "termnavi", "termnavi")
            .map(|dir| dir.config_dir().to_path_buf())
            .unwrap_or_else(|| PathBuf::from("."));
        
        std::fs::create_dir_all(&config_dir)?;
        Ok(config_dir.join("config.toml"))
    }
}
