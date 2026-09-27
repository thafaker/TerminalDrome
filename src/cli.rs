use clap::Parser;
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(
    name = "terminaldrome",
    author,
    version,
    about = "A TUI client for Subsonic and Bandcamp music servers",
    long_about = None,
    after_help = "Run 'terminaldrome --about' for a longer description of the \
                  project, its author and its license."
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

    /// Longer description of the project, the author and the license
    #[arg(long)]
    pub about: bool,
}

/// The text printed by `--about`.
///
/// Kept next to the flag definition so that the option and its output cannot
/// drift apart. The version comes from Cargo.toml and is therefore never out
/// of date.
pub fn about_text() -> String {
    format!(
        r#"TerminalDrome {version}

A lightweight, terminal-based music client (TUI) for the open-source music
server Navidrome, and for any other server that speaks the Subsonic API —
including Bandcamp.

Written in the Rust programming language and designed by developer Jan Montag.
TerminalDrome is aimed specifically at terminal users and self-hosting
enthusiasts who want to manage and play their own music libraries straight from
the command line, without relying on resource-intensive web browsers or
Electron applications.

What it does
  • Browse artists, albums, songs and playlists on your own server
  • Play through mpv, with ASCII cover art and an optional audio visualizer
  • Create playlists, add and remove tracks, shuffle, jukebox / party mode
  • Favourite songs, scrobble plays and keep a local play counter

Project  https://terminaldrome.de
Source   https://github.com/thafaker/terminaldrome
License  MIT

(C) Jan Montag
"#,
        version = env!("CARGO_PKG_VERSION")
    )
}
