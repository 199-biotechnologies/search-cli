use crate::context::AppContext;
use crate::errors::SearchError;
use crate::types::SearchResult;
use async_trait::async_trait;
use serde_json::json;
use std::sync::Arc;
use std::time::Duration;

pub struct Serper {
    ctx: Arc<AppContext>,
}

impl Serper {
    pub fn new(ctx: Arc<AppContext>) -> Self {
        Self { ctx }
    }

    fn api_key(&self) -> &str {
        &self.ctx.config.keys.serper
    }

    async fn query_endpoint(
        &self,
        endpoint: &str,
        query: &str,
        count: usize,
    ) -> Result<serde_json::Value, SearchError> {
        if self.api_key().is_empty() {
            return Err(SearchError::AuthMissing { provider: "serper" });
        }

        let url = format!("https://google.serper.dev/{endpoint}");
        let resp = self
            .ctx
            .client
            .post(&url)
            .header("X-API-KEY", self.api_key())
            .header("Content-Type", "application/json")
            .json(&json!({ "q": query, "num": count }))
            .send()
            .await?;

        if resp.status() == 429 {
            return Err(SearchError::RateLimited { provider: "serper" });
        }
        if !resp.status().is_success() {
            return Err(SearchError::Api {
                provider: "serper",
                code: "api_error",
                message: format!("HTTP {}", resp.status()),
            });
        }

        Ok(resp.json().await?)
    }
}

fn parse_organic(body: &serde_json::Value, source: &str) -> Vec<SearchResult> {
    let key = match source {
        "serper_news" => "news",
        "serper_images" => "images",
        "serper_places" => "places",
        "serper_scholar" | "serper_patents" => "organic",
        _ => "organic",
    };

    let items = body.get(key).and_then(|v| v.as_array());
    items
        .map(|arr| {
            arr.iter()
                .filter_map(|item| {
                    let title = item
                        .get("title")
                        .and_then(|v| v.as_str())
                        .unwrap_or_default()
                        .to_string();
                    let url = item
                        .get("link")
                        .and_then(|v| v.as_str())
                        .unwrap_or_default()
                        .to_string();
                    let snippet = item
                        .get("snippet")
                        .and_then(|v| v.as_str())
                        .unwrap_or_default()
                        .to_string();
                    let published = item.get("date").and_then(|v| v.as_str()).map(String::from);
                    let image_url = item
                        .get("imageUrl")
                        .and_then(|v| v.as_str())
                        .map(String::from);

                    Some(SearchResult {
                        title,
                        url,
                        snippet,
                        source: source.to_string(),
                        published,
                        image_url,
                        extra: None,
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

#[async_trait]
impl super::Provider for Serper {
    fn name(&self) -> &'static str {
        "serper"
    }

    fn capabilities(&self) -> &[&'static str] {
        &[
            "general", "news", "scholar", "patents", "images", "places",
        ]
    }

    fn is_configured(&self) -> bool {
        !self.api_key().is_empty()
    }

    fn timeout(&self) -> Duration {
        Duration::from_secs(10)
    }

    async fn search(&self, query: &str, count: usize) -> Result<Vec<SearchResult>, SearchError> {
        let body = self.query_endpoint("search", query, count).await?;
        Ok(parse_organic(&body, "serper"))
    }

    async fn search_news(
        &self,
        query: &str,
        count: usize,
    ) -> Result<Vec<SearchResult>, SearchError> {
        let body = self.query_endpoint("news", query, count).await?;
        Ok(parse_organic(&body, "serper_news"))
    }
}

impl Serper {
    pub async fn search_scholar(
        &self,
        query: &str,
        count: usize,
    ) -> Result<Vec<SearchResult>, SearchError> {
        let body = self.query_endpoint("scholar", query, count).await?;
        Ok(parse_organic(&body, "serper_scholar"))
    }

    pub async fn search_patents(
        &self,
        query: &str,
        count: usize,
    ) -> Result<Vec<SearchResult>, SearchError> {
        let body = self.query_endpoint("patents", query, count).await?;
        Ok(parse_organic(&body, "serper_patents"))
    }

    pub async fn search_images(
        &self,
        query: &str,
        count: usize,
    ) -> Result<Vec<SearchResult>, SearchError> {
        let body = self.query_endpoint("images", query, count).await?;
        Ok(parse_organic(&body, "serper_images"))
    }

    pub async fn search_places(
        &self,
        query: &str,
        count: usize,
    ) -> Result<Vec<SearchResult>, SearchError> {
        let body = self.query_endpoint("places", query, count).await?;
        Ok(parse_organic(&body, "serper_places"))
    }
}
