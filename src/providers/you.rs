use crate::context::AppContext;
use crate::errors::SearchError;
use crate::providers::augment_query;
use crate::types::{map_freshness, SearchOpts, SearchResult};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;
use std::sync::Arc;

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

    async fn do_search(&self, query: &str, count: usize, opts: &SearchOpts, include_news: bool) -> Result<Vec<SearchResult>, SearchError> {
        if self.api_key().is_empty() {
            return Err(SearchError::AuthMissing { provider: "you" });
        }

        let q = augment_query(query, opts);
        let mut req = self
            .ctx
            .client
            .get("https://ydc-index.io/v1/search")
            .header("X-API-Key", self.api_key())
            .query(&[("query", q.as_str()), ("count", &count.to_string()), ("country", "US"), ("safesearch", "moderate")]);

        if let Some(f) = opts.freshness.as_deref().map(map_freshness) {
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

    async fn search(&self, query: &str, count: usize, opts: &SearchOpts) -> Result<Vec<SearchResult>, SearchError> {
        self.do_search(query, count, opts, false).await
    }

    async fn search_news(&self, query: &str, count: usize, opts: &SearchOpts) -> Result<Vec<SearchResult>, SearchError> {
        self.do_search(query, count, opts, true).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_you_response_deserialize_hits_only() {
        let json = r#"{"hits":[{"title":"Rust","url":"https://rust-lang.org","snippet":"Systems language","score":0.95}]}"#;
        let resp: YouResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.hits.unwrap().len(), 1);
        assert!(resp.news.is_none());
    }

    #[test]
    fn test_you_response_deserialize_news_only() {
        let json = r#"{"news":[{"title":"Breaking","url":"https://news.example","description":"Update","age":"2h"}]}"#;
        let resp: YouResponse = serde_json::from_str(json).unwrap();
        assert!(resp.hits.is_none());
        assert_eq!(resp.news.unwrap().len(), 1);
    }

    #[test]
    fn test_you_response_deserialize_empty() {
        let json = r#"{}"#;
        let resp: YouResponse = serde_json::from_str(json).unwrap();
        assert!(resp.hits.is_none());
        assert!(resp.news.is_none());
    }

    #[test]
    fn test_you_hit_optional_fields() {
        // Minimal hit with all fields optional
        let json = r#"{"hits":[{}]}"#;
        let resp: YouResponse = serde_json::from_str(json).unwrap();
        let hit = &resp.hits.unwrap()[0];
        assert!(hit.title.is_none());
        assert!(hit.url.is_none());
        assert!(hit.snippet.is_none());
        assert!(hit.score.is_none());
    }

    #[test]
    fn test_you_news_optional_fields() {
        let json = r#"{"news":[{}]}"#;
        let resp: YouResponse = serde_json::from_str(json).unwrap();
        let item = &resp.news.unwrap()[0];
        assert!(item.title.is_none());
        assert!(item.url.is_none());
        assert!(item.description.is_none());
        assert!(item.age.is_none());
    }
}
