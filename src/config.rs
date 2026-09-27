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
    println!("📈 Optional: {} to make the Shift+E visualizer",
        cava_install_hint());
    println!("   react to the music instead of a demo.\n");

    Ok(())
}

/// Maps a distribution ID to the command that installs cava.
///
/// Distributions sharing a package manager inherit it through `ID_LIKE`, so
/// Pop, Mint and Elementary resolve to apt without being listed here. An
/// unrecognised distribution yields `None` rather than a guess: naming the
/// wrong package manager sends the user chasing something that does not
/// exist, which is worse than naming none.
///
/// Kept deliberately small and conservative. Only entries whose package
/// manager for `cava` is known are listed.
fn cava_command_for(id: &str, id_like: &[String]) -> Option<&'static str> {
    let by_id = |d: &str| match d {
        "arch" | "manjaro" | "endeavouros" | "garuda" | "cachyos" => {
            Some("sudo pacman -S cava")
        }
        "debian" | "ubuntu" => Some("sudo apt install cava"),
        "fedora" | "nobara" | "rhel" | "centos" => Some("sudo dnf install cava"),
        "nixos" => Some("nix profile install nixpkgs#cava"),
        _ => None,
    };
    by_id(id).or_else(|| id_like.iter().find_map(|d| by_id(d)))
}

/// Pulls `ID` and `ID_LIKE` out of an os-release file's contents.
fn parse_os_release(content: &str) -> (Option<String>, Vec<String>) {
    let mut id = None;
    let mut id_like = Vec::new();
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') { continue; }
        let Some((key, value)) = line.split_once('=') else { continue; };
        let value = value.trim().trim_matches('"');
        match key.trim() {
            "ID"       => id = Some(value.to_ascii_lowercase()),
            "ID_LIKE"  => id_like = value.split_whitespace()
                .map(|d| d.trim_matches('"').to_ascii_lowercase()).collect(),
            _ => {}
        }
    }
    (id, id_like)
}

/// The command that installs cava here, if we can name it with confidence.
pub fn cava_install_command() -> Option<String> {
    // macOS has no os-release; Homebrew is the only sane answer.
    if cfg!(target_os = "macos") { return Some("brew install cava".to_string()); }
    let release = std::fs::read_to_string("/etc/os-release").ok()?;
    let (id, id_like) = parse_os_release(&release);
    cava_command_for(&id?, &id_like).map(str::to_string)
}

/// A one-line instruction for installing cava on this platform.
pub fn cava_install_hint() -> String {
    match cava_install_command() {
        Some(cmd) => format!("install cava ({cmd})"),
        // Unknown distribution, or a platform we have no advice for.
        None => "install cava with your package manager".to_string(),
    }
}

#[cfg(test)]
mod install_hint_tests {
    use super::{cava_command_for, cava_install_hint, parse_os_release};

    /// The "or everything else is Arch" shortcut was wrong: pacman is Arch
    /// only, and the Linux world is far wider than two families.
    #[test]
    fn each_package_manager_is_matched_to_its_own_distributions() {
        let like = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let cases: &[(&str, &[&str], &str)] = &[
            ("arch",   &[],                       "sudo pacman -S cava"),
            ("manjaro", &["arch"],                "sudo pacman -S cava"),
            ("debian", &[],                       "sudo apt install cava"),
            ("ubuntu", &["debian"],               "sudo apt install cava"),
            ("fedora", &[],                       "sudo dnf install cava"),
            ("nixos",  &[],                       "nix profile install nixpkgs#cava"),
        ];
        for (id, id_like, expected) in cases {
            assert_eq!(
                cava_command_for(id, &like(id_like)), Some(*expected),
                "{id} with ID_LIKE {id_like:?}"
            );
        }
    }

    /// Derivatives must inherit their package manager instead of being listed
    /// one by one — that is what the Debian family is: endless.
    #[test]
    fn unknown_derivatives_inherit_via_id_like() {
        for id in ["pop", "linuxmint", "elementary", "zorin", "kali", "mx"] {
            let like = vec!["ubuntu".to_string(), "debian".to_string()];
            assert_eq!(
                cava_command_for(id, &like), Some("sudo apt install cava"),
                "{id} did not inherit apt"
            );
        }
    }

    /// Guessing is worse than staying quiet: an unknown distribution must not
    /// be handed a command that does not exist on it.
    #[test]
    fn unknown_distributions_are_never_guessed() {
        for id in ["weirdlinux", "slackware", "gentoo", "void", "alpine"] {
            assert_eq!(cava_command_for(id, &[]), None, "{id} got a made-up command");
            let like = vec!["weirdbase".to_string()];
            assert_eq!(cava_command_for(id, &like), None, "{id} got a made-up command");
        }
    }

    /// Real os-release files quote values and carry comments; mis-parsing them
    /// would silently fall back to the unhelpful generic hint.
    #[test]
    fn os_release_parsing_survives_real_world_formatting() {
        let content = "# pretty name
NAME=\"Pop!_OS\"
ID=pop
ID_LIKE=\"ubuntu debian\"
VERSION_ID=22.04
";
        let (id, like) = parse_os_release(content);
        assert_eq!(id.as_deref(), Some("pop"));
        assert_eq!(like, vec!["ubuntu", "debian"]);
        assert_eq!(cava_command_for(&id.unwrap(), &like), Some("sudo apt install cava"));
    }

    #[test]
    fn a_broken_os_release_yields_no_guess() {
        for content in ["", "ID
", "=value", "# only comments"] {
            let (id, like) = parse_os_release(content);
            if let Some(id) = id {
                assert_eq!(cava_command_for(&id, &like), None, "guessed from {content:?}");
            }
        }
    }

    /// The hint must always name cava, and must never suggest an empty command.
    #[test]
    fn the_hint_is_always_actionable() {
        let hint = cava_install_hint();
        assert!(hint.contains("cava"), "hint does not name the program: {hint:?}");
        assert!(!hint.contains("()"), "hint has an empty command: {hint:?}");
        assert!(!hint.contains("install  "), "hint has a dangling command: {hint:?}");
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

