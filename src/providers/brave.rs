use crate::context::AppContext;
use crate::errors::SearchError;
use crate::types::SearchResult;
use async_trait::async_trait;
use serde::Deserialize;
use std::sync::Arc;
use std::time::Duration;

pub struct Brave {
    ctx: Arc<AppContext>,
}

impl Brave {
    pub fn new(ctx: Arc<AppContext>) -> Self {
        Self { ctx }
    }

    fn api_key(&self) -> &str {
        &self.ctx.config.keys.brave
    }
}

#[derive(Deserialize)]
struct BraveResponse {
    web: Option<BraveWeb>,
    news: Option<BraveNews>,
}

#[derive(Deserialize)]
struct BraveWeb {
    results: Vec<BraveResult>,
}

#[derive(Deserialize)]
struct BraveNews {
    results: Vec<BraveNewsResult>,
}

#[derive(Deserialize)]
struct BraveResult {
    title: Option<String>,
    url: Option<String>,
    description: Option<String>,
}

#[derive(Deserialize)]
struct BraveNewsResult {
    title: Option<String>,
    url: Option<String>,
    description: Option<String>,
    age: Option<String>,
}

#[async_trait]
impl super::Provider for Brave {
    fn name(&self) -> &'static str {
        "brave"
    }

    fn capabilities(&self) -> &[&'static str] {
        &["general", "news"]
    }

    fn is_configured(&self) -> bool {
        !self.api_key().is_empty()
    }

    fn timeout(&self) -> Duration {
        Duration::from_secs(10)
    }

    async fn search(&self, query: &str, count: usize) -> Result<Vec<SearchResult>, SearchError> {
        if !self.is_configured() {
            return Err(SearchError::AuthMissing { provider: "brave" });
        }

        let resp = self
            .ctx
            .client
            .get("https://api.search.brave.com/res/v1/web/search")
            .header("X-Subscription-Token", self.api_key())
            .header("Accept", "application/json")
            .query(&[("q", query), ("count", &count.to_string())])
            .send()
            .await?;

        if resp.status() == 429 {
            return Err(SearchError::RateLimited { provider: "brave" });
        }
        if !resp.status().is_success() {
            return Err(SearchError::Api {
                provider: "brave",
                code: "api_error",
                message: format!("HTTP {}", resp.status()),
            });
        }

        let body: BraveResponse = resp.json().await?;
        let results = body
            .web
            .map(|w| w.results)
            .unwrap_or_default()
            .into_iter()
            .map(|r| SearchResult {
                title: r.title.unwrap_or_default(),
                url: r.url.unwrap_or_default(),
                snippet: r.description.unwrap_or_default(),
                source: "brave".to_string(),
                published: None,
                image_url: None,
                extra: None,
            })
            .collect();

        Ok(results)
    }

    async fn search_news(
        &self,
        query: &str,
        count: usize,
    ) -> Result<Vec<SearchResult>, SearchError> {
        if !self.is_configured() {
            return Err(SearchError::AuthMissing { provider: "brave" });
        }

        let resp = self
            .ctx
            .client
            .get("https://api.search.brave.com/res/v1/news/search")
            .header("X-Subscription-Token", self.api_key())
            .header("Accept", "application/json")
            .query(&[("q", query), ("count", &count.to_string())])
            .send()
            .await?;

        if !resp.status().is_success() {
            return Err(SearchError::Api {
                provider: "brave",
                code: "api_error",
                message: format!("HTTP {}", resp.status()),
            });
        }

        let body: BraveResponse = resp.json().await?;
        let results = body
            .news
            .map(|n| n.results)
            .unwrap_or_default()
            .into_iter()
            .map(|r| SearchResult {
                title: r.title.unwrap_or_default(),
                url: r.url.unwrap_or_default(),
                snippet: r.description.unwrap_or_default(),
                source: "brave_news".to_string(),
                published: r.age,
                image_url: None,
                extra: None,
            })
            .collect();

        Ok(results)
    }
}
