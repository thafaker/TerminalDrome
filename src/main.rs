mod api;
mod config;

use crate::api::get_artists;
use crate::config::AppConfig;
use anyhow::Result;
use std::collections::HashMap;
use std::io::{self, Write};

fn main() -> Result<()> {
    let config = AppConfig::load()?;

    println!("Starte Anfrage an {} als {}", config.server_url, config.username);
    print!("Lade Künstler...");
    io::stdout().flush()?;

    let mut params: HashMap<&str, &str> = HashMap::new();
    params.insert("u", &config.username[..]);
    params.insert("p", &config.password[..]);
    params.insert("v", "1.16.1");
    params.insert("c", "termnavi");
    params.insert("f", "json");

    let artists = get_artists(&config)?;
    println!("\r{} Künstler erhalten", artists.len());

    for artist in artists.iter().take(10) {
        println!("- {}", artist.name);
    }

    Ok(())
}
