pub mod models;
pub mod endpoints;

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

pub fn build_stream_url(song_id: &str, config: &Config) -> String {
    let auth = AuthParams::new(config);
    format!(
        "{}/rest/stream?id={}&u={}&t={}&s={}&v=1.16.1&c=TerminalDrome&f=json",
        config.server.url, song_id, auth.user, auth.token, auth.salt,
    )
}
