use serde::Deserialize;
use std::fs;

#[derive(Debug, Deserialize)]
pub struct AppConfig {
    pub server_url: String,
    pub username: String,
    pub password: String,
}

impl AppConfig {
    pub fn load() -> Result<Self, Box<dyn std::error::Error>> {
        let config_data = fs::read_to_string("config.toml")?;
        let config: AppConfig = toml::de::from_str(&config_data)?;
        Ok(config)
    }
}
