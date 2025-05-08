mod api;
mod config;

use crate::api::get_artists;
use crate::config::AppConfig;
use anyhow::Result;
use std::io::{self, Write};

fn main() -> Result<()> {
    let config = AppConfig::load()?;

    println!("Starte Anfrage an {} als {}", config.server_url, config.username);
    print!("Lade Künstler...");
    io::stdout().flush()?;

    let artists = get_artists(&config)?;
    println!("\r{} Künstler erhalten", artists.len());

    for artist in artists.iter().take(10) {
        println!("- {}", artist.name);
    }

    Ok(())
}