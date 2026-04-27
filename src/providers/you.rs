use crate::context::AppContext;
use crate::errors::SearchError;
use crate::types::{SearchOpts, SearchResult};
use async_trait::async_trait;
use std::sync::Arc;
use std::time::Duration;

pub struct You {
    ctx: Arc<AppContext>,
}

impl You {
    pub fn new(ctx: Arc<AppContext>) -> Self {
        Self { ctx }
    }

    fn api_key(&self) -> String {
        let from_config = self.ctx.config.keys.you.trim().to_string();
        if !from_config.is_empty() {
            return from_config;
        }
        std::env::var("YDC_API_KEY")
            .or_else(|_| std::env::var("YOU_API_KEY"))
            .or_else(|_| std::env::var("SEARCH_KEYS_YOU"))
            .unwrap_or_default()
    }

    async fn query_endpoint(&self, query: &str, count: usize, opts: &SearchOpts) -> Result<serde_json::Value, SearchError> {
        let mut req = self
            .ctx
            .client
            .get("https://api.you.com/v1/agents/search")
            .query(&[("query", query), ("count", &count.to_string())]);

        if let Some(freshness) = &opts.freshness {
            req = req.query(&[("freshness", freshness)]);
        }

        if !opts.include_domains.is_empty() {
            req = req.query(&[("include_domains", opts.include_domains.join(","))]);
        }
        if !opts.exclude_domains.is_empty() {
            req = req.query(&[("exclude_domains", opts.exclude_domains.join(","))]);
        }

        let api_key = self.api_key();
        if !api_key.is_empty() {
            req = req.header("X-API-Key", api_key);
        }

        super::retry_request(|| {
            let req = req.try_clone().ok_or_else(|| SearchError::Api {
                provider: "you",
                code: "request_clone_failed",
                message: "failed to clone request builder".to_string(),
            });
            async {
                let req = req?;
                let resp = req.send().await?;

                if resp.status() == 429 {
                    return Err(SearchError::RateLimited { provider: "you" });
                }
                if !resp.status().is_success() {
                    let status = resp.status();
                    let body = resp.text().await.unwrap_or_default();
                    return Err(SearchError::Api {
                        provider: "you",
                        code: "api_error",
                        message: format!("HTTP {}: {}", status, body),
                    });
                }

                let body_bytes = resp.bytes().await?;
                let mut body_vec = body_bytes.to_vec();
                simd_json::from_slice(&mut body_vec).map_err(|e| SearchError::Api {
                    provider: "you",
                    code: "json_error",
                    message: e.to_string(),
                })
            }
        })
        .await
    }
}

fn parse_items(arr: Option<&Vec<serde_json::Value>>, source: &str) -> Vec<SearchResult> {
    arr.map(|items| {
        items
            .iter()
            .map(|item| {
                let title = item
                    .get("title")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string();
                let url = item
                    .get("url")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string();
                let snippet = item
                    .get("description")
                    .and_then(|v| v.as_str())
                    .or_else(|| {
                        item.get("snippets")
                            .and_then(|v| v.as_array())
                            .and_then(|s| s.first())
                            .and_then(|v| v.as_str())
                    })
                    .unwrap_or_default()
                    .to_string();
                let published = item.get("page_age").and_then(|v| v.as_str()).map(String::from);
                let image_url = item
                    .get("thumbnail_url")
                    .and_then(|v| v.as_str())
                    .map(String::from);
                SearchResult {
                    title,
                    url,
                    snippet,
                    source: source.to_string(),
                    published,
                    image_url,
                    extra: None,
                }
            })
            .filter(|r| !r.url.is_empty())
            .collect()
    })
    .unwrap_or_default()
}

#[async_trait]
impl super::Provider for You {
    fn name(&self) -> &'static str {
        "you"
    }

    fn capabilities(&self) -> &[&'static str] {
        &["general", "news"]
    }

    fn env_keys(&self) -> &[&'static str] {
        &["YDC_API_KEY", "YOU_API_KEY", "SEARCH_KEYS_YOU"]
    }

    fn is_configured(&self) -> bool {
        true
    }

    fn timeout(&self) -> Duration {
        Duration::from_secs(10)
    }

    async fn search(&self, query: &str, count: usize, opts: &SearchOpts) -> Result<Vec<SearchResult>, SearchError> {
        let body = self.query_endpoint(query, count, opts).await?;
        let web = body
            .get("results")
            .and_then(|v| v.get("web"))
            .and_then(|v| v.as_array());
        Ok(parse_items(web, "you"))
    }

    async fn search_news(
        &self,
        query: &str,
        count: usize,
        opts: &SearchOpts,
    ) -> Result<Vec<SearchResult>, SearchError> {
        let body = self.query_endpoint(query, count, opts).await?;
        let news = body
            .get("results")
            .and_then(|v| v.get("news"))
            .and_then(|v| v.as_array());
        Ok(parse_items(news, "you_news"))
    }
}
