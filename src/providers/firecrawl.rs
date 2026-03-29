use crate::context::AppContext;
use crate::errors::SearchError;
use crate::types::{SearchOpts, SearchResult};
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

    fn api_key(&self) -> String {
        super::resolve_key(&self.ctx.config.keys.firecrawl, "FIRECRAWL_API_KEY")
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

    fn env_keys(&self) -> &[&'static str] { &["FIRECRAWL_API_KEY", "SEARCH_KEYS_FIRECRAWL"] }
    fn capabilities(&self) -> &[&'static str] {
        &["general", "scrape", "extract"]
    }

    fn is_configured(&self) -> bool {
        !self.api_key().is_empty()
    }

    fn timeout(&self) -> Duration {
        Duration::from_secs(30)
    }

    async fn search(&self, query: &str, count: usize, _opts: &SearchOpts) -> Result<Vec<SearchResult>, SearchError> {
        if self.api_key().is_empty() {
            return Err(SearchError::AuthMissing { provider: "firecrawl" });
        }
        // Firecrawl v2 search endpoint — web search + scrape combo
        let client = &self.ctx.client;
        let auth = format!("Bearer {}", self.api_key());
        let body = json!({
            "query": query,
            "limit": count,
            "scrapeOptions": { "formats": ["markdown"] }
        });

        super::retry_request(|| async {
            let resp = client
                .post("https://api.firecrawl.dev/v2/search")
                .header("Authorization", &auth)
                .json(&body)
                .send()
                .await?;

            if resp.status() == 429 {
                return Err(SearchError::RateLimited { provider: "firecrawl" });
            }
            if !resp.status().is_success() {
                return Err(SearchError::Api {
                    provider: "firecrawl",
                    code: "api_error",
                    message: format!("HTTP {}", resp.status()),
                });
            }

            let val: serde_json::Value = resp.json().await?;
            let results = val.get("data")
                .and_then(|d| d.get("web"))
                .or_else(|| val.get("data"))
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter().map(|item| SearchResult {
                        title: item.get("title").and_then(|v| v.as_str()).unwrap_or_default().to_string(),
                        url: item.get("url").and_then(|v| v.as_str()).unwrap_or_default().to_string(),
                        snippet: item.get("description")
                            .or_else(|| item.get("markdown"))
                            .and_then(|v| v.as_str())
                            .unwrap_or_default()
                            .to_string(),
                        source: "firecrawl_search".to_string(),
                        published: None,
                        image_url: None,
                        extra: None,
                    }).collect()
                })
                .unwrap_or_default();

            Ok(results)
        }).await
    }

    async fn search_news(
        &self,
        _query: &str,
        _count: usize,
        _opts: &SearchOpts,
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

        let client = &self.ctx.client;
        let auth = format!("Bearer {}", self.api_key());
        let body = json!({
            "url": url,
            "formats": ["markdown"]
        });

        super::retry_request(|| async {
            let resp = client
                .post("https://api.firecrawl.dev/v2/scrape")
                .header("Authorization", &auth)
                .header("Content-Type", "application/json")
                .json(&body)
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
        })
        .await
    }
}
