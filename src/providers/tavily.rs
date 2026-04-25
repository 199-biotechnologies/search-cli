use crate::context::AppContext;
use crate::errors::SearchError;
use crate::types::{SearchOpts, SearchResult};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;
use std::sync::Arc;

pub struct Tavily {
    ctx: Arc<AppContext>,
}

impl Tavily {
    pub fn new(ctx: Arc<AppContext>) -> Self {
        Self { ctx }
    }

    fn api_key(&self) -> String {
        super::resolve_key(&self.ctx.config.keys.tavily, "TAVILY_API_KEY")
    }

    async fn do_search(
        &self,
        query: &str,
        count: usize,
        topic: &str,
        opts: &SearchOpts,
    ) -> Result<Vec<SearchResult>, SearchError> {
        if self.api_key().is_empty() {
            return Err(SearchError::AuthMissing { provider: "tavily" });
        }

        let mut body = json!({
            "api_key": self.api_key(),
            "query": query,
            "search_depth": "advanced",
            "topic": topic,
            "max_results": count,
            "include_answer": "basic",
            "include_raw_content": false,
        });
        if !opts.include_domains.is_empty() {
            body["include_domains"] = json!(opts.include_domains);
        }
        if !opts.exclude_domains.is_empty() {
            body["exclude_domains"] = json!(opts.exclude_domains);
        }
        // Tavily time_range: day, week, month, year
        if let Some(f) = &opts.freshness {
            body["time_range"] = json!(f);
        }

        let client = &self.ctx.client;
        let resp = super::retry_request(|| async {
            let r = client
                .post("https://api.tavily.com/search")
                .json(&body)
                .send()
                .await?;
            if r.status() == 429 {
                return Err(SearchError::RateLimited { provider: "tavily" });
            }
            if !r.status().is_success() {
                return Err(SearchError::Api {
                    provider: "tavily",
                    code: "api_error",
                    message: format!("HTTP {}", r.status()),
                });
            }
            Ok(r.json::<TavilyResponse>().await?)
        })
        .await?;

        let source = if topic == "news" { "tavily_news" } else { "tavily" };
        let mut results = Vec::new();

        // Prepend the AI-generated answer if available
        if let Some(answer) = resp.answer {
            if !answer.is_empty() {
                results.push(SearchResult {
                    title: "AI Answer".to_string(),
                    url: "tavily://answer".to_string(),
                    snippet: answer,
                    source: format!("{source}_answer"),
                    published: None,
                    image_url: None,
                    extra: None,
                });
            }
        }

        results.extend(resp.results.into_iter().map(|r| SearchResult {
            title: r.title.unwrap_or_default(),
            url: r.url.unwrap_or_default(),
            snippet: r.content.unwrap_or_default(),
            source: source.to_string(),
            published: r.published_time,
            image_url: None,
            extra: r.score.map(|s| json!({"score": s})),
        }));
        Ok(results)
    }
}

#[derive(Deserialize)]
struct TavilyResponse {
    results: Vec<TavilyResult>,
    answer: Option<String>,
}

#[derive(Deserialize)]
struct TavilyResult {
    title: Option<String>,
    url: Option<String>,
    content: Option<String>,
    score: Option<f64>,
    published_time: Option<String>,
}

#[async_trait]
impl super::Provider for Tavily {
    fn name(&self) -> &'static str { "tavily" }
    fn capabilities(&self) -> &[&'static str] { &["general", "news", "academic", "deep"] }
    fn env_keys(&self) -> &[&'static str] { &["TAVILY_API_KEY", "SEARCH_KEYS_TAVILY"] }
    fn is_configured(&self) -> bool { !self.api_key().is_empty() }

    async fn search(&self, query: &str, count: usize, opts: &SearchOpts) -> Result<Vec<SearchResult>, SearchError> {
        self.do_search(query, count, "general", opts).await
    }

    async fn search_news(&self, query: &str, count: usize, opts: &SearchOpts) -> Result<Vec<SearchResult>, SearchError> {
        self.do_search(query, count, "news", opts).await
    }
}
