// src/api/mod.rs
use reqwest::{Client, Url};
use serde::de::DeserializeOwned;
use std::collections::HashMap;

#[derive(Clone)]
pub struct NavidromeClient {
    client: Client,
    base_url: Url,
    auth_params: HashMap<String, String>,
}

impl NavidromeClient {
    pub fn new(config: &crate::config::AppConfig) -> Result<Self, anyhow::Error> {
        let client = Client::new();
        let base_url = Url::parse(&config.server.url)?;
        
        let token = format!("{:x}", md5::compute(format!(
            "{}:{}", 
            config.server.username, 
            config.server.password
        )));

        let mut auth_params = HashMap::new();
        auth_params.insert("u".into(), config.server.username.clone());
        auth_params.insert("t".into(), token);
        auth_params.insert("s".into(), "termnavi".into());
        auth_params.insert("v".into(), "1.16.1".into());
        auth_params.insert("c".into(), "termnavi".into());
        auth_params.insert("f".into(), "json".into());

        Ok(Self {
            client,
            base_url,
            auth_params,
        })
    }

    async fn get<T: DeserializeOwned>(&self, endpoint: &str, params: Option<HashMap<String, String>>) -> Result<T, anyhow::Error> {
        let mut url = self.base_url.join(endpoint)?;
        
        // Basis-Auth-Parameter hinzufügen
        for (k, v) in &self.auth_params {
            url.query_pairs_mut().append_pair(k, v);
        }

        // Zusätzliche Parameter hinzufügen
        if let Some(params) = params {
            for (k, v) in params {
                url.query_pairs_mut().append_pair(&k, &v);
            }
        }

        let response = self.client.get(url).send().await?;
        Ok(response.json().await?)
    }
}