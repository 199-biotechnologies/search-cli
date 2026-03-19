use crate::context::AppContext;
use crate::errors::SearchError;
use crate::types::SearchResult;
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;
use std::sync::Arc;
use std::time::Duration;

pub struct Exa {
    ctx: Arc<AppContext>,
}

impl Exa {
    pub fn new(ctx: Arc<AppContext>) -> Self {
        Self { ctx }
    }

    fn api_key(&self) -> &str {
        &self.ctx.config.keys.exa
    }

    async fn post_api(
        &self,
        path: &str,
        body: serde_json::Value,
    ) -> Result<ExaResponse, SearchError> {
        if self.api_key().is_empty() {
            return Err(SearchError::AuthMissing { provider: "exa" });
        }

        let url = format!("https://api.exa.ai/{path}");
        let resp = self
            .ctx
            .client
            .post(&url)
            .header("x-api-key", self.api_key())
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await?;

        if resp.status() == 429 {
            return Err(SearchError::RateLimited { provider: "exa" });
        }
        if !resp.status().is_success() {
            return Err(SearchError::Api {
                provider: "exa",
                code: "api_error",
                message: format!("HTTP {}", resp.status()),
            });
        }

        Ok(resp.json().await?)
    }
}

#[derive(Deserialize)]
struct ExaResponse {
    results: Option<Vec<ExaResult>>,
}

#[derive(Deserialize)]
struct ExaResult {
    title: Option<String>,
    url: Option<String>,
    text: Option<String>,
    #[serde(rename = "publishedDate")]
    published_date: Option<String>,
}

fn to_results(exa: ExaResponse, source: &str) -> Vec<SearchResult> {
    exa.results
        .unwrap_or_default()
        .into_iter()
        .map(|r| SearchResult {
            title: r.title.unwrap_or_default(),
            url: r.url.unwrap_or_default(),
            snippet: r.text.unwrap_or_default(),
            source: source.to_string(),
            published: r.published_date,
            image_url: None,
            extra: None,
        })
        .collect()
}

#[async_trait]
impl super::Provider for Exa {
    fn name(&self) -> &'static str {
        "exa"
    }

    fn capabilities(&self) -> &[&'static str] {
        &["general", "academic", "people", "similar", "deep"]
    }

    fn is_configured(&self) -> bool {
        !self.api_key().is_empty()
    }

    fn timeout(&self) -> Duration {
        Duration::from_secs(15)
    }

    async fn search(&self, query: &str, count: usize) -> Result<Vec<SearchResult>, SearchError> {
        let body = json!({
            "query": query,
            "numResults": count,
            "type": "auto",
            "contents": { "text": true }
        });
        let resp = self.post_api("search", body).await?;
        Ok(to_results(resp, "exa"))
    }

    async fn search_news(
        &self,
        query: &str,
        count: usize,
    ) -> Result<Vec<SearchResult>, SearchError> {
        let body = json!({
            "query": query,
            "numResults": count,
            "type": "auto",
            "category": "news",
            "contents": { "text": true }
        });
        let resp = self.post_api("search", body).await?;
        Ok(to_results(resp, "exa_news"))
    }
}

impl Exa {
    pub async fn search_people(
        &self,
        query: &str,
        count: usize,
    ) -> Result<Vec<SearchResult>, SearchError> {
        let body = json!({
            "query": query,
            "numResults": count,
            "type": "auto",
            "category": "linkedin profile",
            "contents": { "text": true }
        });
        let resp = self.post_api("search", body).await?;
        Ok(to_results(resp, "exa_people"))
    }

    pub async fn find_similar(
        &self,
        url: &str,
        count: usize,
    ) -> Result<Vec<SearchResult>, SearchError> {
        let body = json!({
            "url": url,
            "numResults": count,
            "contents": { "text": true }
        });
        let resp = self.post_api("findSimilar", body).await?;
        Ok(to_results(resp, "exa_similar"))
    }
}
