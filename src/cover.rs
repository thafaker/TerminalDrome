
use std::{collections::HashMap, io::Cursor, sync::Mutex};
use anyhow::Result;
use image::{imageops::{colorops::grayscale, FilterType}, io::Reader as ImageReader};

use crate::api::{build_auth_query, models::Album};
use crate::config::Config;

lazy_static! {
    pub static ref COVER_CACHE: Mutex<HashMap<String, String>> = Mutex::new(HashMap::new());
}

pub async fn get_ascii_cover(album: Option<&Album>, config: &Config) -> String {
    let Some(album)    = album else { return default_cover_art(); };
    let Some(cover_id) = &album.cover_art else { return default_cover_art(); };

    {
        let cache = COVER_CACHE.lock().unwrap();
        if let Some(cached) = cache.get(cover_id) {
            return cached.clone();
        }
    }

    match fetch_cover_art(cover_id, config).await {
        Ok(img_data) => {
            let ascii = image_to_ascii(&img_data, 30).unwrap_or_else(|_| default_cover_art());
            COVER_CACHE.lock().unwrap().insert(cover_id.clone(), ascii.clone());
            ascii
        }
        Err(e) => {
            eprintln!("Error loading cover art: {}", e);
            default_cover_art()
        }
    }
}

async fn fetch_cover_art(cover_id: &str, config: &Config) -> Result<Vec<u8>> {
    let mut params = build_auth_query(config);
    params.push(("id".to_string(), cover_id.to_string()));
    let response = reqwest::Client::new()
        .get(format!("{}/rest/getCoverArt", config.server.url))
        .query(&params).send().await?;
    Ok(response.bytes().await?.to_vec())
}

pub fn image_to_ascii(img_data: &[u8], width: u32) -> Result<String> {
    let height = (width as f32 / 2.2) as u32;
    let img = ImageReader::new(Cursor::new(img_data))
        .with_guessed_format()?.decode()?
        .resize_exact(width, height, FilterType::Triangle);
    let grayscale  = grayscale(&img);
    let chars      = [" ", "░", "▒", "▓", "█", "@", "#", "S", "%", "?", "*", "+", ";", ":", ",", "."];
    let img_width  = grayscale.width() as usize;
    let mut ascii  = String::with_capacity((width * height) as usize);
    ascii.push_str(&" ".repeat(img_width));
    ascii.push('\n');
    for y in 0..grayscale.height() {
        let mut line = String::with_capacity(img_width);
        for x in 0..grayscale.width() {
            let pixel      = grayscale.get_pixel(x, y);
            let brightness = pixel[0] as f32 / 255.0;
            let adjusted   = brightness.powf(1.8);
            let index      = (adjusted * (chars.len() - 1) as f32).round() as usize;
            line.push_str(chars[index]);
        }
        let pad = img_width.saturating_sub(line.chars().count());
        line.push_str(&" ".repeat(pad));
        ascii.push_str(&line);
        ascii.push('\n');
    }
    Ok(ascii)
}

pub fn default_cover_art() -> String {
    r#"
   ___
  / __\_____   _____ _ __
 / /  / _ \ \ / / _ \ '__|
/ /__| (_) \ V /  __/ |
\____/\___/ \_/ \___|_|
  /\  /\___ _ __ ___
 / /_/ / _ \ '__/ _ \
/ __  /  __/ | |  __/
\/ /_/ \___|_|  \___|
    "#.to_string()
}
