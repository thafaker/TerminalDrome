use anyhow::Result;
use directories::ProjectDirs;
use serde::Deserialize;
use std::path::Path;

#[derive(Debug, Deserialize, Clone)]
pub struct Config {
    pub server: ServerConfig,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ServerConfig {
    pub url:      String,
    pub username: String,
    pub password: String,
}

pub fn read_config() -> Result<Config> {
    let config_name = "config.toml";

    let local_path = Path::new(config_name);
    if local_path.exists() {
        return parse_config(local_path);
    }

    if let Some(proj_dirs) = ProjectDirs::from("com", "TerminalDrome", "TerminalDrome") {
        let config_path = proj_dirs.config_dir().join(config_name);
        if config_path.exists() {
            return parse_config(&config_path);
        }
    }

    let error_msg = format!(
        "Config file not found!\n\n\
        Required paths:\n\
        - {}\n\
        - ./config.toml\n\n\
        Config template:\n{}",
        ProjectDirs::from("com", "TerminalDrome", "TerminalDrome")
            .map(|d| d.config_dir().join(config_name).display().to_string())
            .unwrap_or_else(|| "~/.config/TerminalDrome/config.toml".to_string()),
        "[server]\nurl = \"https://your-navidrome-server.com\"\nusername = \"your_username\"\npassword = \"your_password\""
    );
    anyhow::bail!("{}", error_msg)
}

fn parse_config(path: &Path) -> Result<Config> {
    let content = std::fs::read_to_string(path)?;
    Ok(toml::from_str(&content)?)
}
