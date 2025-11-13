use reqwest::Url;
use serde::{Deserialize, Serialize};

use super::types::EncodedRecommendation;

#[derive(Debug, Clone)]
pub struct ChaosClient {
    base_url: Url,
    api_key: Option<String>,
    http: reqwest::Client,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RecommendationsRequest<'a> {
    protocol: &'a str,
    #[serde(rename = "updateType")]
    update_type: Vec<&'a str>,
}

impl ChaosClient {
    pub fn try_new(base_url: &str, api_key: Option<String>) -> crate::Result<Self> {
        let base_url = Url::parse(base_url).map_err(crate::Error::custom)?;
        Ok(Self {
            base_url,
            api_key,
            http: reqwest::Client::new(),
        })
    }

    pub fn from_env() -> crate::Result<Self> {
        let base = std::env::var("CHAOS_BASE_URL")
            .unwrap_or_else(|_| "https://oracle.chaoslabs.co".to_string());
        let api_key = std::env::var("CHAOS_API_KEY").ok();
        Self::try_new(&base, api_key)
    }

    pub async fn fetch_latest_recommendations(
        &self,
        protocol: &str,
        update_types: &[&str],
    ) -> crate::Result<Vec<EncodedRecommendation>> {
        let url = self
            .base_url
            .join("/edge/risk-oracle/recommendations/latest")
            .map_err(crate::Error::custom)?;

        let mut req = self
            .http
            .post(url)
            .json(&RecommendationsRequest {
                protocol,
                update_type: update_types.to_vec(),
            })
            .header("Content-Type", "application/json");

        if let Some(key) = &self.api_key {
            req = req.header("Authorization", key);
        }

        let resp = req.send().await.map_err(crate::Error::custom)?;
        let status = resp.status();
        let text = resp.text().await.map_err(crate::Error::custom)?;

        if !status.is_success() {
            let snippet: String = text.chars().take(1024).collect();
            return Err(crate::Error::custom(format!(
                "chaos oracle http error: {status} body: {snippet}"
            )));
        }

        match serde_json::from_str::<Vec<EncodedRecommendation>>(&text) {
            Ok(body) => Ok(body),
            Err(err) => {
                let snippet: String = text.chars().take(1024).collect();
                Err(crate::Error::custom(format!(
                    "failed to decode recommendations: {err} body_snippet: {snippet}"
                )))
            }
        }
    }
}
