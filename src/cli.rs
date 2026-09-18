use clap::Parser;
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(
    name = "terminaldrome",
    author,
    version,
    about = "Ein TUI Client für Subsonic und Bandcamp Music Server",
    long_about = None
)]
pub struct Cli {
    /// Pfad zur Konfigurationsdatei
    #[arg(short, long, value_name = "FILE")]
    pub config: Option<PathBuf>,

    /// Subsonic Server URL (überschreibt Wert aus Config)
    #[arg(short, long, value_name = "URL")]
    pub server: Option<String>,

    /// Benutzername (überschreibt Wert aus Config)
    #[arg(short, long, value_name = "USERNAME")]
    pub user: Option<String>,
}
