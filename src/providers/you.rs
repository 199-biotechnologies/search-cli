use crate::context::AppContext;
use crate::errors::SearchError;
use crate::types::{SearchOpts, SearchResult};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;
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
        super::resolve_key(&self.ctx.config.keys.you, "YOU_API_KEY")
    }

    fn map_freshness(f: &str) -> &str {
        match f {
            "day" => "pd",
            "week" => "pw",
            "month" => "pm",
            "year" => "py",
            other => other,
        }
    }

    fn augment_query(query: &str, opts: &SearchOpts) -> String {
        let mut q = query.to_string();
        for d in &opts.include_domains {
            q = format!("{q} site:{d}");
        }
        for d in &opts.exclude_domains {
            q = format!("{q} -site:{d}");
        }
        q
    }

    async fn do_search(&self, query: &str, count: usize, opts: &SearchOpts, include_news: bool) -> Result<Vec<SearchResult>, SearchError> {
        if self.api_key().is_empty() {
            return Err(SearchError::AuthMissing { provider: "you" });
        }

        let q = Self::augment_query(query, opts);
        let mut req = self
            .ctx
            .client
            .get("https://ydc-index.io/v1/search")
            .header("X-API-Key", self.api_key())
            .query(&[("query", q.as_str()), ("count", &count.to_string()), ("country", "US"), ("safesearch", "moderate")]);

        if let Some(f) = opts.freshness.as_deref().map(Self::map_freshness) {
            req = req.query(&[("freshness", f)]);
        }

        let resp = super::retry_request(|| {
            let req = req.try_clone().ok_or_else(|| SearchError::Config("failed to clone request".into()));
            async move {
                let req = req?;
                let r = req.send().await?;
                if r.status() == 429 {
                    return Err(SearchError::RateLimited { provider: "you" });
                }
                if !r.status().is_success() {
                    return Err(SearchError::Api {
                        provider: "you",
                        code: "api_error",
                        message: format!("HTTP {}", r.status()),
                    });
                }
                Ok(r.json::<YouResponse>().await?)
            }
        }).await?;

        let mut out = Vec::new();
        for hit in resp.hits.unwrap_or_default() {
            out.push(SearchResult {
                title: hit.title.unwrap_or_default(),
                url: hit.url.unwrap_or_default(),
                snippet: hit.snippet.unwrap_or_default(),
                source: "you".to_string(),
                published: None,
                image_url: None,
                extra: hit.score.map(|s| json!({"score": s})),
            });
        }

        if include_news {
            for item in resp.news.unwrap_or_default() {
                out.push(SearchResult {
                    title: item.title.unwrap_or_default(),
                    url: item.url.unwrap_or_default(),
                    snippet: item.description.unwrap_or_default(),
                    source: "you_news".to_string(),
                    published: item.age,
                    image_url: None,
                    extra: None,
                });
            }
        }

        Ok(out)
    }
}

#[derive(Deserialize)]
struct YouResponse {
    hits: Option<Vec<YouHit>>,
    news: Option<Vec<YouNews>>,
}

#[derive(Deserialize)]
struct YouHit {
    title: Option<String>,
    url: Option<String>,
    snippet: Option<String>,
    score: Option<f64>,
}

#[derive(Deserialize)]
struct YouNews {
    title: Option<String>,
    url: Option<String>,
    description: Option<String>,
    age: Option<String>,
}

#[async_trait]
impl super::Provider for You {
    fn name(&self) -> &'static str { "you" }
    fn capabilities(&self) -> &[&'static str] { &["general", "news", "deep"] }
    fn env_keys(&self) -> &[&'static str] { &["YOU_API_KEY", "SEARCH_KEYS_YOU"] }
    fn is_configured(&self) -> bool { !self.api_key().is_empty() }
    fn timeout(&self) -> Duration { Duration::from_secs(12) }

    async fn search(&self, query: &str, count: usize, opts: &SearchOpts) -> Result<Vec<SearchResult>, SearchError> {
        self.do_search(query, count, opts, false).await
    }

    async fn search_news(&self, query: &str, count: usize, opts: &SearchOpts) -> Result<Vec<SearchResult>, SearchError> {
        self.do_search(query, count, opts, true).await
    }
}
