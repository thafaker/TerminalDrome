use clap::Parser;
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(
    name = "terminaldrome",
    author,
    version,
    about = "A TUI client for Subsonic and Bandcamp music servers",
    long_about = None
)]
pub struct Cli {
    /// Path to the configuration file
    #[arg(short, long, value_name = "FILE")]
    pub config: Option<PathBuf>,

    /// Subsonic server URL (overrides config file value)
    #[arg(short, long, value_name = "URL")]
    pub server: Option<String>,

    /// Username (overrides config file value)
    #[arg(short, long, value_name = "USERNAME")]
    pub user: Option<String>,
}
