use serde::Deserialize;
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Deserialize)]
pub struct AppConfig {
    pub server_url: String,
    pub username: String,
    pub password: String,
}

impl AppConfig {
    pub fn load() -> anyhow::Result<Self> {
        let config_dir = directories::ProjectDirs::from("de", "apfelhammer", "termnavi")
            .ok_or_else(|| anyhow::anyhow!("Could not determine config directory"))?;

        let config_file = config_dir.config_dir().join("config.toml");

        let content = fs::read_to_string(&config_file)?;
        let config: AppConfig = toml::from_str(&content)?;
        Ok(config)
    }
}