use crate::context::AppContext;
use crate::errors::SearchError;
use crate::types::{SearchOpts, SearchResult};
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

    fn api_key(&self) -> String {
        super::resolve_key(&self.ctx.config.keys.exa, "EXA_API_KEY")
    }

    async fn post_api(&self, path: &str, body: serde_json::Value) -> Result<ExaResponse, SearchError> {
        if self.api_key().is_empty() {
            return Err(SearchError::AuthMissing { provider: "exa" });
        }

        let url = format!("https://api.exa.ai/{path}");
        let client = &self.ctx.client;
        let api_key = self.api_key();

        super::retry_request(|| async {
            let resp = client
                .post(&url)
                .header("x-api-key", api_key.as_str())
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

            let body_bytes = resp.bytes().await?;
            let mut body_vec = body_bytes.to_vec();
            simd_json::from_slice(&mut body_vec).map_err(|e| SearchError::Api {
                provider: "exa",
                code: "json_error",
                message: e.to_string(),
            })
        })
        .await
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
    highlights: Option<Vec<String>>,
}

fn to_results(exa: ExaResponse, source: &str) -> Vec<SearchResult> {
    exa.results
        .unwrap_or_default()
        .into_iter()
        .map(|r| {
            // Prefer highlights over full text for snippet (more relevant, shorter)
            let snippet = if let Some(ref hl) = r.highlights {
                if !hl.is_empty() {
                    hl.join(" ... ")
                } else {
                    r.text.clone().unwrap_or_default()
                }
            } else {
                r.text.unwrap_or_default()
            };
            SearchResult {
                title: r.title.unwrap_or_default(),
                url: r.url.unwrap_or_default(),
                snippet,
                source: source.to_string(),
                published: r.published_date,
                image_url: None,
                extra: None,
            }
        })
        .collect()
}

fn build_search_body(query: &str, count: usize, opts: &SearchOpts) -> serde_json::Value {
    let mut body = json!({
        "query": query,
        "numResults": count,
        "type": "auto",
        "contents": {
            "text": true,
            "highlights": true
        }
    });
    // Exa supports includeDomains / excludeDomains natively
    if !opts.include_domains.is_empty() {
        body["includeDomains"] = json!(opts.include_domains);
    }
    if !opts.exclude_domains.is_empty() {
        body["excludeDomains"] = json!(opts.exclude_domains);
    }
    // Freshness → startPublishedDate (ISO 8601)
    if let Some(ref freshness) = opts.freshness {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let days_back: u64 = match freshness.as_str() {
            "day" => 1,
            "week" => 7,
            "month" => 30,
            "year" => 365,
            _ => 0,
        };
        if days_back > 0 {
            let target = now.saturating_sub(days_back * 86400);
            // Format as ISO 8601: YYYY-MM-DDTHH:MM:SSZ
            let z = (target / 86400) as i64 + 719468;
            let era = if z >= 0 { z } else { z - 146096 } / 146097;
            let doe = (z - era * 146097) as u64;
            let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
            let y = yoe as i64 + era * 400;
            let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
            let mp = (5 * doy + 2) / 153;
            let d = doy - (153 * mp + 2) / 5 + 1;
            let m = if mp < 10 { mp + 3 } else { mp - 9 };
            let y = if m <= 2 { y + 1 } else { y };
            body["startPublishedDate"] = json!(format!("{y:04}-{m:02}-{d:02}T00:00:00Z"));
        }
    }
    body
}

#[async_trait]
impl super::Provider for Exa {
    fn name(&self) -> &'static str { "exa" }
    fn capabilities(&self) -> &[&'static str] { &["general", "academic", "people", "similar", "deep"] }
    fn env_keys(&self) -> &[&'static str] { &["EXA_API_KEY", "SEARCH_KEYS_EXA"] }
    fn is_configured(&self) -> bool { !self.api_key().is_empty() }
    fn timeout(&self) -> Duration { Duration::from_secs(15) }

    async fn search(&self, query: &str, count: usize, opts: &SearchOpts) -> Result<Vec<SearchResult>, SearchError> {
        let body = build_search_body(query, count, opts);
        let resp = self.post_api("search", body).await?;
        Ok(to_results(resp, "exa"))
    }

    async fn search_news(&self, query: &str, count: usize, opts: &SearchOpts) -> Result<Vec<SearchResult>, SearchError> {
        let mut body = build_search_body(query, count, opts);
        body["category"] = json!("news");
        let resp = self.post_api("search", body).await?;
        Ok(to_results(resp, "exa_news"))
    }
}

impl Exa {
    pub async fn search_people(&self, query: &str, count: usize) -> Result<Vec<SearchResult>, SearchError> {
        let body = json!({
            "query": query, "numResults": count, "type": "auto",
            "category": "people", "contents": { "text": true }
        });
        let resp = self.post_api("search", body).await?;
        Ok(to_results(resp, "exa_people"))
    }

    pub async fn find_similar(&self, url: &str, count: usize) -> Result<Vec<SearchResult>, SearchError> {
        let body = json!({ "url": url, "numResults": count, "contents": { "text": true } });
        let resp = self.post_api("findSimilar", body).await?;
        Ok(to_results(resp, "exa_similar"))
    }
}
