pub mod models;
pub mod endpoints;

// Single Source of Truth: MusicSource lebt in endpoints.rs und wird hier re-exportiert.
pub use endpoints::MusicSource;
use crate::api::endpoints::build_auth_query_for_source;

use crate::config::Config;
use rand::Rng;

pub struct AuthParams {
    pub user:  String,
    pub token: String,
    pub salt:  String,
}

impl AuthParams {
    pub fn new(config: &Config) -> Self {
        let salt: String = rand::thread_rng()
            .sample_iter(&rand::distributions::Alphanumeric)
            .take(8)
            .map(char::from)
            .collect();
        let token = format!("{:x}", md5::compute(format!("{}{}", config.server.password, salt)));
        Self { user: config.server.username.clone(), token, salt }
    }
}

/// Legacy-Token-Auth gegen `config.server` (Navidrome).
///
/// Wird aktuell nur von `build_stream_url` benötigt, falls wir später
/// Navidrome auf Token-Auth umstellen. Solange `build_stream_url` die
/// source-aware Variante aus `endpoints.rs` nutzt, ist diese Funktion
/// ungenutzt.
#[allow(dead_code)]
pub fn build_auth_query(config: &Config) -> Vec<(String, String)> {
    let auth = AuthParams::new(config);
    vec![
        ("u".to_string(), auth.user),
        ("t".to_string(), auth.token),
        ("s".to_string(), auth.salt),
        ("v".to_string(), "1.16.1".to_string()),
        ("c".to_string(), "TerminalDrome".to_string()),
        ("f".to_string(), "json".to_string()),
    ]
}

/// Baut die Stream-URL für einen Song bei der angegebenen Quelle.
/// Wichtig: `source` bestimmt Zielserver UND Auth-Parameter.
pub fn build_stream_url(song_id: &str, source: MusicSource, config: &Config) -> String {
    let target = endpoints::get_target_config(source, config);
    let params = endpoints::build_auth_query_for_source(source, config);
    let query: String = params
        .iter()
        .map(|(k, v)| format!("{}={}", k, v))
        .collect::<Vec<_>>()
        .join("&");
    format!("{}/rest/stream?id={}&{}", target.url, song_id, query)
}

use anyhow::{bail, Result};
use reqwest::Client;

pub async fn check_connection(config: &Config) -> Result<()> {
    let client = Client::new();
    let auth_params = build_auth_query_for_source(MusicSource::Navidrome, config);
    let ping_url = format!("{}/rest/ping.view", config.server.url.trim_end_matches('/'));

    let response = client
        .get(&ping_url)
        .query(&auth_params)
        .send()
        .await?;

    if !response.status().is_success() {
        bail!("Server responded with status code: {}", response.status());
    }

    let body: serde_json::Value = response.json().await?;
    
    // Subsonic API gibt bei Fehlern "status": "failed" im JSON zurück
    if let Some(status) = body.get("subsonic-response").and_then(|r| r.get("status")) {
        if status == "failed" {
            let error_code = body["subsonic-response"]["error"]["code"].as_i64().unwrap_or(0);
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