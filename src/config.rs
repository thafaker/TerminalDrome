use anyhow::{Context, Result};
use rpassword::read_password;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::{self, Write};
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use md5;

pub fn generate_token_and_salt(password: &str) -> (String, String) {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .subsec_nanos();
    let salt = format!("{:x}", nanos);

    // md5::compute gibt direkt den Hash zurück
    let digest = md5::compute(format!("{}{}", password, salt));
    let token = format!("{:x}", digest);

    (token, salt)
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct Config {
    pub server: ServerConfig,
    #[serde(default)]
    pub bandcamp: Option<ServerConfig>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct ServerConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    pub url: String,
    pub username: String,
    #[serde(default)]
    pub password: Option<String>,
    #[serde(default)]
    pub token: Option<String>,
    #[serde(default)]
    pub salt: Option<String>,
}

fn default_true() -> bool {
    true
}

pub fn get_config_path() -> PathBuf {
    if Path::new("config.toml").exists() {
        return PathBuf::from("config.toml");
    }

    if let Some(config_dir) = dirs::config_dir() {
        let app_dir = config_dir.join("terminaldrome");
        let _ = fs::create_dir_all(&app_dir);
        app_dir.join("config.toml")
    } else {
        PathBuf::from("config.toml")
    }
}

pub fn read_config() -> Result<Config> {
    let path = get_config_path();
    if !path.exists() {
        return Ok(Config::default());
    }

    let content = fs::read_to_string(&path)
        .with_context(|| format!("Konnte Config-Datei nicht lesen: {:?}", path))?;
    let config: Config = toml::from_str(&content)
        .with_context(|| "Fehler beim Parsen der config.toml")?;
    Ok(config)
}

pub fn save_config(config: &Config) -> Result<()> {
    let path = get_config_path();

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let mut config_to_serialize = config.clone();
    let is_bandcamp_active = config
        .bandcamp
        .as_ref()
        .map_or(false, |bc| bc.enabled);

    if !is_bandcamp_active {
        config_to_serialize.bandcamp = None;
    }

    let mut content = toml::to_string_pretty(&config_to_serialize)?;

    if !is_bandcamp_active {
        let bandcamp_commented = match &config.bandcamp {
            Some(bc) => format!(
                "\n# [bandcamp]\n# enabled = false\n# url = \"{}\"\n# username = \"{}\"\n# token = \"{}\"\n# salt = \"{}\"\n",
                bc.url,
                bc.username,
                bc.token.as_deref().unwrap_or(""),
                bc.salt.as_deref().unwrap_or("")
            ),
            None => "\n# [bandcamp]\n# enabled = false\n# url = \"https://bandcamp.com/api/subsonic\"\n# username = \"hier_eintragen\"\n# token = \"hier_eintragen\"\n# salt = \"hier_eintragen\"\n".to_string(),
        };
        content.push_str(&bandcamp_commented);
    }

    fs::write(&path, content)?;

    #[cfg(unix)]
    {
        let permissions = fs::Permissions::from_mode(0o600);
        fs::set_permissions(&path, permissions)?;
    }

    Ok(())
}

pub fn setup_initial_credentials(config: &mut Config) -> Result<()> {
    println!("⚙️ Erstkonfiguration für TerminalDrome\n");

    let default_url = if config.server.url.is_empty() || config.server.url.contains("example.com") {
        "https://music.apfelhammer.de"
    } else {
        &config.server.url
    };

    print!("Server URL [{}]: ", default_url);
    io::stdout().flush()?;
    let mut url_input = String::new();
    io::stdin().read_line(&mut url_input)?;
    let url_input = url_input.trim();

    let raw_url = if url_input.is_empty() {
        default_url
    } else {
        url_input
    };

    config.server.enabled = true;
    config.server.url = if !raw_url.starts_with("http://") && !raw_url.starts_with("https://") {
        format!("https://{}", raw_url)
    } else {
        raw_url.to_string()
    };

    print!("Benutzername: ");
    io::stdout().flush()?;
    let mut user_input = String::new();
    io::stdin().read_line(&mut user_input)?;
    let user_input = user_input.trim();

    if !user_input.is_empty() {
        config.server.username = user_input.to_string();
    }

    if config.server.username.is_empty() {
        anyhow::bail!("Kein Benutzername eingegeben.");
    }

    print!("Passwort für '{}': ", config.server.username);
    io::stdout().flush()?;
    let password = read_password()?;
    if password.trim().is_empty() {
        anyhow::bail!("Kein Passwort eingegeben.");
    }

    // Passwort sofort in Token & Salt umwandeln und Passwort-Feld leeren
    let (token, salt) = generate_token_and_salt(password.trim());
    config.server.token = Some(token);
    config.server.salt = Some(salt);
    config.server.password = None;

    if config.bandcamp.is_none() {
        config.bandcamp = Some(ServerConfig {
            enabled: false,
            url: "https://bandcamp.com/api/subsonic".to_string(),
            username: "hier_eintragen".to_string(),
            password: None,
            token: Some("hier_eintragen".to_string()),
            salt: Some("hier_eintragen".to_string()),
        });
    }

    save_config(config)?;

    let path = get_config_path();
    println!("\n✅ Zugangsdaten sicher als Token/Salt gespeichert in: {:?}", path);
    println!("🔒 Dateirechte wurden auf 600 gesetzt.");
    println!("💡 Das Klartext-Passwort wurde nicht auf der Festplatte gespeichert.\n");

    Ok(())
}