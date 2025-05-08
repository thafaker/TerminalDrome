use serde::Deserialize;
use std::{fs, error::Error, fmt};

#[derive(Debug)]
pub struct ConfigError(String);

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Config error: {}", self.0)
    }
}

impl Error for ConfigError {}

impl From<std::io::Error> for ConfigError {
    fn from(err: std::io::Error) -> Self {
        ConfigError(format!("IO error: {}", err))
    }
}

impl From<toml::de::Error> for ConfigError {
    fn from(err: toml::de::Error) -> Self {
        ConfigError(format!("TOML parsing error: {}", err))
    }
}

#[derive(Debug, Deserialize, Clone)]
pub struct ServerConfig {
    pub url: String,
    pub username: String,
    pub password: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct AppConfig {
    pub server: ServerConfig,
}

impl AppConfig {
    pub fn load() -> Result<Self, ConfigError> {
        let config_data = fs::read_to_string("config.toml")?;
        let config: AppConfig = toml::from_str(&config_data)?;
        Ok(config)
    }
}