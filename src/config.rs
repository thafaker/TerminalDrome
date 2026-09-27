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

    // md5::compute returns the hash directly
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

/// Defaults to `true` so that existing config files without an `enabled` key
/// keep working. Only the optional `[bandcamp]` section honours this flag.
fn default_enabled() -> bool {
    true
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ServerConfig {
    /// Allows switching an optional source (currently only `[bandcamp]`) off
    /// without deleting its credentials.
    #[serde(default = "default_enabled")]
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

/// Hand-written so that `enabled` starts out as `true`; the derived `Default`
/// would hand out `false` and silently disable an otherwise valid source.
impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            enabled:   default_enabled(),
            url:       String::new(),
            username:  String::new(),
            password:  None,
            token:     None,
            salt:      None,
        }
    }
}

impl ServerConfig {
    /// Returns true if this server is switched on *and* has real, usable
    /// credentials. Placeholders like `your_username` or empty tokens count as
    /// "not configured".
    pub fn is_configured(&self) -> bool {
        if !self.enabled {
            return false;
        }

        if self.url.trim().is_empty() {
            return false;
        }

        if self.username.is_empty()
            || self.username == "your_username"
            || self.username == "hier_eintragen"
        {
            return false;
        }

        let has_token = self
            .token
            .as_deref()
            .map_or(false, |t| !t.is_empty() && t != "your_token" && t != "hier_eintragen");
        let has_pass = self
            .password
            .as_deref()
            .map_or(false, |p| !p.is_empty() && p != "your_password" && p != "hier_eintragen");

        has_token || has_pass
    }
}

pub fn get_config_path() -> PathBuf {
    // ich hasse rust, ich begreife es nie
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
        .with_context(|| format!("Could not read config file: {:?}", path))?;
    let config: Config = toml::from_str(&content)
        .with_context(|| "Failed to parse config.toml")?;
    Ok(config)
}

pub fn save_config(config: &Config) -> Result<()> {
    let path = get_config_path();

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let mut config_to_serialize = config.clone();

    // Only serialize the Bandcamp section if it actually has usable credentials.
    // Otherwise it gets appended as a commented-out template below.
    let is_bandcamp_active = config
        .bandcamp
        .as_ref()
        .map_or(false, |bc| bc.is_configured());

    if !is_bandcamp_active {
        config_to_serialize.bandcamp = None;
    }

    let mut content = toml::to_string_pretty(&config_to_serialize)?;

    if !is_bandcamp_active {
        let bandcamp_commented = match &config.bandcamp {
            Some(bc) => {
                // Migrate legacy German placeholder values on write so users
                // who never enabled Bandcamp don't keep stale strings around.
                let username = if bc.username == "hier_eintragen" {
                    "your_username".to_string()
                } else {
                    bc.username.clone()
                };
                let token = match bc.token.as_deref() {
                    Some("hier_eintragen") | None => "your_token".to_string(),
                    Some(t) => t.to_string(),
                };
                let salt = match bc.salt.as_deref() {
                    Some("hier_eintragen") | None => "your_salt".to_string(),
                    Some(s) => s.to_string(),
                };
                format!(
                    "\n# [bandcamp]\n# enabled = true\n# url = \"{}\"\n# username = \"{}\"\n# token = \"{}\"\n# salt = \"{}\"\n",
                    bc.url, username, token, salt
                )
            }
            None => "\n# [bandcamp]\n# enabled = true\n# url = \"https://bandcamp.com/api/subsonic\"\n# username = \"your_username\"\n# token = \"your_token\"\n# salt = \"your_salt\"\n".to_string(),
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
    println!("⚙️  First-time setup for TerminalDrome\n");

    // ── Step 1: Navidrome / Subsonic server ────────────────────────────────
    println!("── Step 1: Your Navidrome or Subsonic server ──\n");

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

    let raw_url = if url_input.is_empty() { default_url } else { url_input };

    config.server.url = if !raw_url.starts_with("http://") && !raw_url.starts_with("https://") {
        format!("https://{}", raw_url)
    } else {
        raw_url.to_string()
    };

    print!("Username: ");
    io::stdout().flush()?;
    let mut user_input = String::new();
    io::stdin().read_line(&mut user_input)?;
    let user_input = user_input.trim();

    if !user_input.is_empty() {
        config.server.username = user_input.to_string();
    }

    if config.server.username.is_empty() {
        anyhow::bail!("No username entered.");
    }

    print!("Password for '{}': ", config.server.username);
    io::stdout().flush()?;
    let password = read_password()?;
    if password.trim().is_empty() {
        anyhow::bail!("No password entered.");
    }

    // Convert the password into a Subsonic token + salt immediately.
    // The plain-text password is never written to disk.
    let (token, salt) = generate_token_and_salt(password.trim());
    config.server.token = Some(token);
    config.server.salt = Some(salt);
    config.server.password = None;

    // ── Step 2: Optional Bandcamp source ───────────────────────────────────
    println!("\n── Step 2: Optional — connect your Bandcamp account ──\n");
    println!("Bandcamp offers a Subsonic-compatible endpoint, so you can use it as");
    println!("a second music source inside TerminalDrome. Credentials are generated");
    println!("separately from your Bandcamp login and can be revoked at any time.");
    println!();
    println!("  1. Open this URL in your browser:");
    println!("     https://bandcamp.com/settings?pane=fan");
    println!("  2. Scroll down to the \"Subsonic\" section.");
    println!("  3. Generate your credentials (a 32-character username and a password).");
    println!();

    print!("Set up Bandcamp now? [y/N]: ");
    io::stdout().flush()?;
    let mut bandcamp_choice = String::new();
    io::stdin().read_line(&mut bandcamp_choice)?;

    if bandcamp_choice.trim().eq_ignore_ascii_case("y") {
        println!();
        print!("Bandcamp Subsonic username: ");
        io::stdout().flush()?;
        let mut bc_user = String::new();
        io::stdin().read_line(&mut bc_user)?;
        let bc_user = bc_user.trim().to_string();

        if bc_user.is_empty() {
            println!("⚠️  No username entered — skipping Bandcamp setup.");
        } else {
            print!("Bandcamp Subsonic password: ");
            io::stdout().flush()?;
            let bc_pass = read_password()?;

            if bc_pass.trim().is_empty() {
                println!("⚠️  No password entered — skipping Bandcamp setup.");
            } else {
                let (bc_token, bc_salt) = generate_token_and_salt(bc_pass.trim());

                config.bandcamp = Some(ServerConfig {
                    enabled:   true,
                    url:       "https://bandcamp.com/api/subsonic".to_string(),
                    username:  bc_user,
                    password:  None,
                    token:     Some(bc_token),
                    salt:      Some(bc_salt),
                });

                println!("\n✅ Bandcamp configured. Press Shift+B inside TerminalDrome to switch sources.");
            }
        }
    } else {
        println!("\n⏭  Skipping Bandcamp setup. You can enable it later by editing config.toml.");
        println!("   See the README for instructions.");
    }

    // ── Save ───────────────────────────────────────────────────────────────
    save_config(config)?;

    let path = get_config_path();
    println!("\n✅ Credentials securely saved as token/salt in: {:?}", path);
    println!("🔒 File permissions set to 600 (owner read/write only).");
    println!("💡 Your plain-text passwords were not stored on disk.");
    println!("📈 Optional: install cava ({}) to make the", cava_install_hint());
    println!("   Shift+E visualizer react to the music instead of a demo.\n");

    Ok(())
}

/// The command that installs cava on the platform we are running on.
///
/// The visualizer works on Linux and macOS, so a hardcoded pacman command
/// would send macOS users to a package manager they do not have.
pub fn cava_install_hint() -> &'static str {
    if cfg!(target_os = "macos") { "brew install cava" } else { "sudo pacman -S cava" }
}

#[cfg(test)]
mod install_hint_tests {
    use super::cava_install_hint;

    /// A macOS user must never be told to run pacman, and a Linux user must
    /// not be sent to a package manager they do not have.
    #[test]
    fn the_hint_matches_the_platform_it_was_built_for() {
        let hint = cava_install_hint();
        let on_macos = cfg!(target_os = "macos");
        assert_eq!(hint.contains("brew"), on_macos, "wrong package manager: {hint:?}");
        assert_eq!(hint.contains("pacman"), !on_macos, "wrong package manager: {hint:?}");
        assert!(hint.contains("cava"), "hint does not name the program: {hint:?}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The `enabled` key is new; configs written by older versions must keep
    /// working, and Bandcamp must stay switched on after an upgrade.
    #[test]
    fn config_without_enabled_key_stays_active() {
        let config: Config = toml::from_str(
            r#"
            [server]
            url      = "https://nav.example"
            username = "joe"
            token    = "abc"
            salt     = "def"

            [bandcamp]
            url      = "https://bandcamp.com/api/subsonic"
            username = "fan"
            token    = "ghi"
            salt     = "jkl"
            "#,
        )
        .unwrap();

        assert!(config.bandcamp.as_ref().unwrap().enabled);
        assert!(config.bandcamp.as_ref().unwrap().is_configured());
    }

    #[test]
    fn enabled_false_switches_bandcamp_off() {
        let config: Config = toml::from_str(
            r#"
            [server]
            url      = "https://nav.example"
            username = "joe"
            token    = "abc"
            salt     = "def"

            [bandcamp]
            enabled  = false
            url      = "https://bandcamp.com/api/subsonic"
            username = "fan"
            token    = "ghi"
            salt     = "jkl"
            "#,
        )
        .unwrap();

        assert!(!config.bandcamp.as_ref().unwrap().is_configured());
    }

    #[test]
    fn placeholders_and_empty_url_are_not_configured() {
        let mut config: Config = toml::from_str(
            r#"
            [server]
            url      = "https://nav.example"
            username = "joe"
            token    = "abc"
            salt     = "def"
            "#,
        )
        .unwrap();

        config.bandcamp = Some(ServerConfig {
            enabled:  true,
            url:      "https://bandcamp.com/api/subsonic".to_string(),
            username: "your_username".to_string(),
            password: None,
            token:    Some("your_token".to_string()),
            salt:     Some("your_salt".to_string()),
        });
        assert!(!config.bandcamp.as_ref().unwrap().is_configured());

        config.bandcamp = Some(ServerConfig {
            username: "fan".to_string(),
            url:      String::new(),
            ..ServerConfig::default()
        });
        assert!(!config.bandcamp.as_ref().unwrap().is_configured());
    }

    /// Mirrors what `save_config` serialises, without touching the filesystem.
    #[test]
    fn serialised_config_round_trips() {
        let config = Config {
            server: ServerConfig {
                enabled:  true,
                url:      "https://nav.example".to_string(),
                username: "joe".to_string(),
                password: None,
                token:    Some("abc".to_string()),
                salt:     Some("def".to_string()),
            },
            bandcamp: Some(ServerConfig {
                enabled:  true,
                url:      "https://bandcamp.com/api/subsonic".to_string(),
                username: "fan".to_string(),
                password: Some("pw".to_string()),
                token:    None,
                salt:     None,
            }),
        };

        let text = toml::to_string_pretty(&config).unwrap();
        let parsed: Config = toml::from_str(&text).unwrap();

        assert_eq!(parsed.bandcamp.as_ref().unwrap().username, "fan");
        assert_eq!(
            parsed.bandcamp.as_ref().unwrap().password.as_deref(),
            Some("pw")
        );
        assert!(parsed.bandcamp.as_ref().unwrap().is_configured());
    }

    #[test]
    fn default_config_keeps_sources_enabled() {
        let config = Config::default();
        assert!(config.server.enabled);
        assert!(config.bandcamp.is_none());
    }
}
