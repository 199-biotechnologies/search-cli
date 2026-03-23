use crate::context::AppContext;
use crate::errors::SearchError;
use crate::types::{SearchOpts, SearchResult};
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
        opts: &SearchOpts,
    ) -> Result<serde_json::Value, SearchError> {
        if self.api_key().is_empty() {
            return Err(SearchError::AuthMissing { provider: "serper" });
        }

        // Serper uses site: in query for domain filtering
        let q = augment_query(query, opts);
        let url = format!("https://google.serper.dev/{endpoint}");
        let client = &self.ctx.client;
        let api_key = self.api_key();

        let mut body = json!({ "q": q, "num": count });
        // Serper freshness via tbs param: qdr:d, qdr:w, qdr:m, qdr:y
        if let Some(f) = &opts.freshness {
            let tbs = match f.as_str() {
                "day" => "qdr:d",
                "week" => "qdr:w",
                "month" => "qdr:m",
                "year" => "qdr:y",
                other => other,
            };
            body["tbs"] = json!(tbs);
        }

        super::retry_request(|| async {
            let resp = client
                .post(&url)
                .header("X-API-KEY", api_key)
                .header("Content-Type", "application/json")
                .json(&body)
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

            let body_bytes = resp.bytes().await?;
            let mut body_vec = body_bytes.to_vec();
            Ok(simd_json::from_slice(&mut body_vec).map_err(|e| SearchError::Api {
                provider: "serper",
                code: "json_error",
                message: e.to_string(),
            })?)
        })
        .await
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
                .map(|item| {
                    let title = item.get("title").and_then(|v| v.as_str()).unwrap_or_default().to_string();
                    let url = item.get("link").and_then(|v| v.as_str()).unwrap_or_default().to_string();
                    let snippet = item.get("snippet").and_then(|v| v.as_str()).unwrap_or_default().to_string();
                    let published = item.get("date").and_then(|v| v.as_str()).map(String::from);
                    let image_url = item.get("imageUrl").and_then(|v| v.as_str()).map(String::from);
                    SearchResult { title, url, snippet, source: source.to_string(), published, image_url, extra: None }
                })
                .collect()
        })
        .unwrap_or_default()
}

#[async_trait]
impl super::Provider for Serper {
    fn name(&self) -> &'static str { "serper" }
    fn capabilities(&self) -> &[&'static str] { &["general", "news", "scholar", "patents", "images", "places"] }
    fn is_configured(&self) -> bool { !self.api_key().is_empty() }
    fn timeout(&self) -> Duration { Duration::from_secs(10) }

    async fn search(&self, query: &str, count: usize, opts: &SearchOpts) -> Result<Vec<SearchResult>, SearchError> {
        let body = self.query_endpoint("search", query, count, opts).await?;
        Ok(parse_organic(&body, "serper"))
    }

    async fn search_news(&self, query: &str, count: usize, opts: &SearchOpts) -> Result<Vec<SearchResult>, SearchError> {
        let body = self.query_endpoint("news", query, count, opts).await?;
        Ok(parse_organic(&body, "serper_news"))
    }
}

impl Serper {
    pub async fn search_scholar(&self, query: &str, count: usize) -> Result<Vec<SearchResult>, SearchError> {
        self.search_special("scholar", query, count).await
    }
    pub async fn search_patents(&self, query: &str, count: usize) -> Result<Vec<SearchResult>, SearchError> {
        self.search_special("patents", query, count).await
    }
    pub async fn search_images(&self, query: &str, count: usize) -> Result<Vec<SearchResult>, SearchError> {
        self.search_special("images", query, count).await
    }
    pub async fn search_places(&self, query: &str, count: usize) -> Result<Vec<SearchResult>, SearchError> {
        self.search_special("places", query, count).await
    }

    async fn search_special(&self, endpoint: &str, query: &str, count: usize) -> Result<Vec<SearchResult>, SearchError> {
        let body = self.query_endpoint(endpoint, query, count, &SearchOpts::default()).await?;
        Ok(parse_organic(&body, &format!("serper_{endpoint}")))
    }
}
