use crate::config::Config;
use anyhow::{bail, Result};
use reqwest::Client;

pub mod endpoints;
pub mod models;

pub use endpoints::*;

pub fn build_stream_url(song_id: &str, source: MusicSource, config: &Config) -> String {
    let target = endpoints::get_target_config(source, config);
    let params = endpoints::build_auth_query_for_source(source, config);
    let query: String = params
        .iter()
        .map(|(k, v)| format!("{}={}", k, v))
        .collect::<Vec<_>>()
        .join("&");
    format!(
        "{}/rest/stream?id={}&{}",
        target.url.trim_end_matches('/'),
        song_id,
        query
    )
}

pub async fn check_connection(config: &Config) -> Result<()> {
    let client = Client::new();
    let auth_params = build_auth_query_for_source(MusicSource::Navidrome, config);
    let ping_url = format!("{}/rest/ping.view", config.server.url.trim_end_matches('/'));

    let response = client.get(&ping_url).query(&auth_params).send().await?;

    if !response.status().is_success() {
        bail!("Server responded with status code: {}", response.status());
    }

    let body: serde_json::Value = response.json().await?;

    if let Some(status) = body.get("subsonic-response").and_then(|r| r.get("status")) {
        if status == "failed" {
            let error_code = body["subsonic-response"]["error"]["code"]
                .as_i64()
                .unwrap_or(0);
            let error_msg = body["subsonic-response"]["error"]["message"]
                .as_str()
                .unwrap_or("Unknown authentication error");

            match error_code {
                40 => bail!("Authentication failed: Wrong username or password/token."),
                10 => bail!("Server protocol version mismatch."),
                _ => bail!("API Error (code {}): {}", error_code, error_msg),
            }
        }
    }

    Ok(())
}
