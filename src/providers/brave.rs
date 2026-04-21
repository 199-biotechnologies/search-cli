use crate::context::AppContext;
use crate::errors::SearchError;
use crate::types::{SearchOpts, SearchResult};
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

    fn api_key(&self) -> String {
        super::resolve_key(&self.ctx.config.keys.brave, "BRAVE_API_KEY")
    }

    fn base_url(&self) -> String {
        std::env::var("BRAVE_BASE_URL")
            .unwrap_or_else(|_| "https://api.search.brave.com".to_string())
            .trim_end_matches('/')
            .to_string()
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
    extra_snippets: Option<Vec<String>>,
}

#[derive(Deserialize)]
struct BraveNewsResult {
    title: Option<String>,
    url: Option<String>,
    description: Option<String>,
    age: Option<String>,
}

/// Brave freshness: pd (day), pw (week), pm (month), py (year)
fn map_freshness(f: &str) -> &str {
    match f {
        "day" => "pd",
        "week" => "pw",
        "month" => "pm",
        "year" => "py",
        other => other, // pass through if already in Brave format
    }
}

/// Append site: operators for domain filtering
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

#[async_trait]
impl super::Provider for Brave {
    fn name(&self) -> &'static str {
        "brave"
    }

    fn capabilities(&self) -> &[&'static str] {
        &["general", "news", "deep"]
    }

    fn env_keys(&self) -> &[&'static str] {
        &["BRAVE_API_KEY", "SEARCH_KEYS_BRAVE"]
    }

    fn is_configured(&self) -> bool {
        !self.api_key().is_empty()
    }

    fn timeout(&self) -> Duration {
        Duration::from_secs(10)
    }

    async fn search(&self, query: &str, count: usize, opts: &SearchOpts) -> Result<Vec<SearchResult>, SearchError> {
        if !self.is_configured() {
            return Err(SearchError::AuthMissing { provider: "brave" });
        }

        let client = &self.ctx.client;
        let api_key = self.api_key();
        let endpoint = format!("{}/res/v1/web/search", self.base_url());
        let count_str = count.to_string();
        let q = augment_query(query, opts);
        let freshness = opts.freshness.as_deref().map(map_freshness);

        super::retry_request(|| async {
            let mut req = client
                .get(&endpoint)
                .header("X-Subscription-Token", api_key.as_str())
                .header("Accept", "application/json")
                .query(&[("q", q.as_str()), ("count", &count_str), ("extra_snippets", "true")]);

            if let Some(f) = freshness {
                req = req.query(&[("freshness", f)]);
            }

            let resp = req.send().await?;

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

            let body_bytes = resp.bytes().await?;
            let mut body_vec = body_bytes.to_vec();
            let body: BraveResponse = simd_json::from_slice(&mut body_vec)
                .map_err(|e| SearchError::Api {
                    provider: "brave",
                    code: "json_error",
                    message: e.to_string(),
                })?;
            let results = body
                .web
                .map(|w| w.results)
                .unwrap_or_default()
                .into_iter()
                .map(|r| {
                    // Combine description with extra snippets for richer context
                    let mut snippet = r.description.unwrap_or_default();
                    if let Some(extras) = r.extra_snippets {
                        for extra in extras {
                            snippet = format!("{snippet}\n{extra}");
                        }
                    }
                    SearchResult {
                        title: r.title.unwrap_or_default(),
                        url: r.url.unwrap_or_default(),
                        snippet,
                        source: "brave".to_string(),
                        published: None,
                        image_url: None,
                        extra: None,
                    }
                })
                .collect();

            Ok(results)
        })
        .await
    }

    async fn search_news(&self, query: &str, count: usize, opts: &SearchOpts) -> Result<Vec<SearchResult>, SearchError> {
        if !self.is_configured() {
            return Err(SearchError::AuthMissing { provider: "brave" });
        }

        let client = &self.ctx.client;
        let api_key = self.api_key();
        let endpoint = format!("{}/res/v1/news/search", self.base_url());
        let count_str = count.to_string();
        let q = augment_query(query, opts);
        let freshness = opts.freshness.as_deref().map(map_freshness);

        super::retry_request(|| async {
            let mut req = client
                .get(&endpoint)
                .header("X-Subscription-Token", api_key.as_str())
                .header("Accept", "application/json")
                .query(&[("q", q.as_str()), ("count", &count_str)]);

            if let Some(f) = freshness {
                req = req.query(&[("freshness", f)]);
            }

            let resp = req.send().await?;

            if !resp.status().is_success() {
                return Err(SearchError::Api {
                    provider: "brave",
                    code: "api_error",
                    message: format!("HTTP {}", resp.status()),
                });
            }

            let body_bytes = resp.bytes().await?;
            let mut body_vec = body_bytes.to_vec();
            let body: BraveResponse = simd_json::from_slice(&mut body_vec)
                .map_err(|e| SearchError::Api {
                    provider: "brave",
                    code: "json_error",
                    message: e.to_string(),
                })?;
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
        })
        .await
    }
}

impl Brave {
    /// LLM Context API — returns pre-extracted, relevance-scored content for RAG/grounding
    pub async fn search_llm_context(&self, query: &str, count: usize, opts: &SearchOpts) -> Result<Vec<SearchResult>, SearchError> {
        if self.api_key().is_empty() {
            return Err(SearchError::AuthMissing { provider: "brave" });
        }

        let client = &self.ctx.client;
        let api_key = self.api_key();
        let endpoint = format!("{}/res/v1/llm/context", self.base_url());
        let q = augment_query(query, opts);
        let count_str = count.to_string();
        let freshness = opts.freshness.as_deref().map(map_freshness);

        super::retry_request(|| async {
            let mut req = client
                .get(&endpoint)
                .header("X-Subscription-Token", api_key.as_str())
                .header("Accept", "application/json")
                .query(&[
                    ("q", q.as_str()),
                    ("count", &count_str),
                    ("maximum_number_of_tokens", "16384"),
                    ("context_threshold_mode", "balanced"),
                ]);

            if let Some(f) = freshness {
                req = req.query(&[("freshness", f)]);
            }

            let resp = req.send().await?;

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

            let body: serde_json::Value = resp.json().await?;
            let mut results = Vec::new();

            // Parse grounding.generic array
            if let Some(generic) = body.pointer("/grounding/generic").and_then(|v| v.as_array()) {
                for item in generic {
                    let url = item.get("url").and_then(|v| v.as_str()).unwrap_or_default();
                    let title = item.get("title").and_then(|v| v.as_str()).unwrap_or_default();
                    let snippets = item.get("snippets")
                        .and_then(|v| v.as_array())
                        .map(|arr| arr.iter()
                            .filter_map(|s| s.as_str())
                            .collect::<Vec<_>>()
                            .join("\n"))
                        .unwrap_or_default();

                    results.push(SearchResult {
                        title: title.to_string(),
                        url: url.to_string(),
                        snippet: snippets,
                        source: "brave_llm_context".to_string(),
                        published: None,
                        image_url: None,
                        extra: None,
                    });
                }
            }

            Ok(results)
        })
        .await
    }
}
