use crate::context::AppContext;
use crate::errors::SearchError;
use crate::types::SearchResult;
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;
use std::sync::Arc;
use std::time::Duration;

pub struct Firecrawl {
    ctx: Arc<AppContext>,
}

impl Firecrawl {
    pub fn new(ctx: Arc<AppContext>) -> Self {
        Self { ctx }
    }

    fn api_key(&self) -> &str {
        &self.ctx.config.keys.firecrawl
    }
}

#[derive(Deserialize)]
struct FirecrawlScrapeResponse {
    data: Option<FirecrawlData>,
}

#[derive(Deserialize)]
struct FirecrawlData {
    markdown: Option<String>,
    metadata: Option<FirecrawlMetadata>,
}

#[derive(Deserialize)]
struct FirecrawlMetadata {
    title: Option<String>,
    #[serde(rename = "sourceURL")]
    source_url: Option<String>,
}

#[async_trait]
impl super::Provider for Firecrawl {
    fn name(&self) -> &'static str {
        "firecrawl"
    }

    fn capabilities(&self) -> &[&'static str] {
        &["scrape", "extract"]
    }

    fn is_configured(&self) -> bool {
        !self.api_key().is_empty()
    }

    fn timeout(&self) -> Duration {
        Duration::from_secs(30)
    }

    async fn search(&self, _query: &str, _count: usize) -> Result<Vec<SearchResult>, SearchError> {
        Ok(vec![]) // Firecrawl is primarily for scraping, not searching
    }

    async fn search_news(
        &self,
        _query: &str,
        _count: usize,
    ) -> Result<Vec<SearchResult>, SearchError> {
        Ok(vec![])
    }
}

impl Firecrawl {
    pub async fn scrape_url(&self, url: &str) -> Result<Vec<SearchResult>, SearchError> {
        if self.api_key().is_empty() {
            return Err(SearchError::AuthMissing {
                provider: "firecrawl",
            });
        }

        let resp = self
            .ctx
            .client
            .post("https://api.firecrawl.dev/v1/scrape")
            .header("Authorization", format!("Bearer {}", self.api_key()))
            .header("Content-Type", "application/json")
            .json(&json!({
                "url": url,
                "formats": ["markdown"]
            }))
            .send()
            .await?;

        if resp.status() == 429 {
            return Err(SearchError::RateLimited {
                provider: "firecrawl",
            });
        }
        if !resp.status().is_success() {
            return Err(SearchError::Api {
                provider: "firecrawl",
                code: "api_error",
                message: format!("HTTP {}", resp.status()),
            });
        }

        let body: FirecrawlScrapeResponse = resp.json().await?;
        if let Some(data) = body.data {
            let meta = data.metadata.unwrap_or(FirecrawlMetadata {
                title: None,
                source_url: None,
            });
            Ok(vec![SearchResult {
                title: meta.title.unwrap_or_default(),
                url: meta.source_url.unwrap_or_else(|| url.to_string()),
                snippet: data.markdown.unwrap_or_default(),
                source: "firecrawl".to_string(),
                published: None,
                image_url: None,
                extra: None,
            }])
        } else {
            Ok(vec![])
        }
    }
}
