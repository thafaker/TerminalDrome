use crate::config::{Config, ServerConfig};
use anyhow::{bail, Result};
use rand::Rng;
use reqwest::Client;

pub mod endpoints;
pub mod models;

pub use endpoints::*;

#[allow(dead_code)]
pub struct AuthParams {
    pub user: String,
    pub token: String,
    pub salt: String,
}

impl AuthParams {
    pub fn new(config: &Config) -> Self {
        let user = config.server.username.clone();

        if let (Some(token), Some(salt)) = (&config.server.token, &config.server.salt) {
            return Self {
                user,
                token: token.clone(),
                salt: salt.clone(),
            };
        }

        let salt: String = rand::thread_rng()
            .sample_iter(&rand::distributions::Alphanumeric)
            .take(8)
            .map(char::from)
            .collect();

        let pass = config.server.password.as_deref().unwrap_or("");
        let digest = md5::compute(format!("{}{}", pass, salt));
        let token = format!("{:x}", digest);

        Self { user, token, salt }
    }
}

pub fn build_auth_params(config: &ServerConfig) -> Vec<(&'static str, String)> {
    let mut params = vec![
        ("u", config.username.clone()),
        ("v", "1.16.1".to_string()),
        ("c", "terminaldrome".to_string()),
        ("f", "json".to_string()),
    ];

    if let (Some(token), Some(salt)) = (&config.token, &config.salt) {
        params.push(("t", token.clone()));
        params.push(("s", salt.clone()));
    } else if let Some(password) = &config.password {
        params.push(("p", password.clone()));
    }

    params
}

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