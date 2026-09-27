use crate::config::Config;
use anyhow::{bail, Result};
use reqwest::Client;

pub mod endpoints;
pub mod models;

pub use endpoints::*;

/// Stream URL handed to mpv/ffmpeg.
///
/// The query is assembled through `Url` rather than by string concatenation so
/// that song ids and credentials get percent-encoded exactly like they do in
/// every other request.
pub fn build_stream_url(song_id: &str, source: MusicSource, config: &Config) -> String {
    let target = endpoints::get_target_config(source, config);
    let base = format!("{}/rest/stream", target.url.trim_end_matches('/'));

    let mut url = match reqwest::Url::parse(&base) {
        Ok(url) => url,
        // A malformed base URL cannot be encoded; hand mpv the raw string so it
        // can report the real problem.
        Err(_) => return format!("{}?id={}", base, song_id),
    };

    {
        let mut query = url.query_pairs_mut();
        query.append_pair("id", song_id);
        for (key, value) in endpoints::build_auth_query_for_source(source, config) {
            query.append_pair(key, &value);
        }
    }

    url.into()
}

/// Ping one source and surface the Subsonic error, if any.
pub async fn check_source_connection(source: MusicSource, config: &Config) -> Result<()> {
    let target = endpoints::get_target_config(source, config);
    let params = endpoints::build_auth_query_for_source(source, config);
    let ping_url = format!("{}/rest/ping.view", target.url.trim_end_matches('/'));

    let response = Client::new()
        .get(&ping_url)
        .query(&params)
        .send()
        .await
        .map_err(|e| anyhow::anyhow!("{} is not reachable: {}", source.label(), e))?;

    let status = response.status();
    let body = response.text().await.unwrap_or_default();

    if !status.is_success() {
        bail!(
            "{} responded with HTTP {}: {}",
            source.label(),
            status,
            body.trim()
        );
    }

    let json: serde_json::Value = serde_json::from_str(&body)
        .map_err(|_| anyhow::anyhow!("{} did not return a JSON response", source.label()))?;

    if json.get("subsonic-response").is_none() {
        bail!(
            "{} did not return a Subsonic response: {}",
            source.label(),
            body.trim()
        );
    }

    if json["subsonic-response"]["status"].as_str() == Some("failed") {
        let code = json["subsonic-response"]["error"]["code"].as_i64().unwrap_or(0);
        let msg = json["subsonic-response"]["error"]["message"]
            .as_str()
            .unwrap_or("Unknown authentication error");

        match code {
            40 => bail!("{}: wrong username or password/token.", source.label()),
            10 => bail!("{}: protocol version mismatch.", source.label()),
            _  => bail!("{}: API error (code {}): {}", source.label(), code, msg),
        }
    }

    Ok(())
}

/// Verify the main server. Failures here are fatal — see `main`.
pub async fn check_connection(config: &Config) -> Result<()> {
    check_source_connection(MusicSource::Navidrome, config).await
}

/// Verify the optional Bandcamp source. Called during startup so that broken
/// Bandcamp credentials are reported up front instead of surfacing later as an
/// unexplained empty collection.
pub async fn check_bandcamp_connection(config: &Config) -> Option<Result<()>> {
    if !endpoints::is_source_usable(MusicSource::Bandcamp, config) {
        return None;
    }
    Some(check_source_connection(MusicSource::Bandcamp, config).await)
}
