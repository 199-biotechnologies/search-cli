use crate::context::AppContext;
use crate::errors::SearchError;
use crate::types::{SearchOpts, SearchResult};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;
use std::sync::Arc;
use std::time::Duration;

pub struct Xai {
    ctx: Arc<AppContext>,
}

impl Xai {
    pub fn new(ctx: Arc<AppContext>) -> Self {
        Self { ctx }
    }

    fn api_key(&self) -> String {
        super::resolve_key(&self.ctx.config.keys.xai, "XAI_API_KEY")
    }

    async fn call_responses_api(
        &self,
        prompt: &str,
        x_search_config: Option<serde_json::Value>,
    ) -> Result<XaiResponse, SearchError> {
        let api_key = self.api_key();
        if api_key.is_empty() {
            return Err(SearchError::AuthMissing { provider: "xai" });
        }

        let mut x_search_tool = json!({"type": "x_search"});
        if let Some(config) = x_search_config {
            if let (Some(tool_obj), Some(config_obj)) =
                (x_search_tool.as_object_mut(), config.as_object())
            {
                for (k, v) in config_obj {
                    tool_obj.insert(k.clone(), v.clone());
                }
            }
        }

        let body = json!({
            "model": "grok-4-1-fast",
            "input": [{"role": "user", "content": prompt}],
            "tools": [x_search_tool],
            "store": false,
        });

        let client = &self.ctx.client;
        let key = api_key;

        // xAI needs its own client with a longer timeout — the global client
        // has a 10s timeout which is too short for LLM inference + tool use
        let xai_client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(90))
            .build()
            .unwrap_or_else(|_| client.clone());

        super::retry_request(|| {
            let body = body.clone();
            let key = key.clone();
            let xai_client = xai_client.clone();
            async move {
                let resp = xai_client
                    .post("https://api.x.ai/v1/responses")
                    .header("Authorization", format!("Bearer {key}"))
                    .header("Content-Type", "application/json")
                    .json(&body)
                    .send()
                    .await?;

                if resp.status() == 401 {
                    return Err(SearchError::AuthMissing { provider: "xai" });
                }
                if resp.status() == 429 {
                    return Err(SearchError::RateLimited { provider: "xai" });
                }
                if !resp.status().is_success() {
                    return Err(SearchError::Api {
                        provider: "xai",
                        code: "api_error",
                        message: format!("HTTP {}", resp.status()),
                    });
                }

                let body_bytes = resp.bytes().await?;
                let mut body_vec = body_bytes.to_vec();
                simd_json::from_slice(&mut body_vec).map_err(|e| SearchError::Api {
                    provider: "xai",
                    code: "json_error",
                    message: e.to_string(),
                })
            }
        })
        .await
    }

    fn build_x_search_config(&self, opts: &SearchOpts) -> Option<serde_json::Value> {
        let mut config = serde_json::Map::new();

        // Map include_domains to allowed_x_handles
        if !opts.include_domains.is_empty() {
            config.insert(
                "allowed_x_handles".to_string(),
                json!(opts.include_domains),
            );
        }

        // Map exclude_domains to excluded_x_handles
        if !opts.exclude_domains.is_empty() {
            config.insert(
                "excluded_x_handles".to_string(),
                json!(opts.exclude_domains),
            );
        }

        // Convert freshness to date range
        if let Some(ref freshness) = opts.freshness {
            let now = chrono_today();
            let from = match freshness.as_str() {
                "day" => subtract_days(&now, 1),
                "week" => subtract_days(&now, 7),
                "month" => subtract_days(&now, 30),
                "year" => subtract_days(&now, 365),
                _ => None,
            };
            if let Some(from_date) = from {
                config.insert("from_date".to_string(), json!(from_date));
                config.insert("to_date".to_string(), json!(now));
            }
        }

        if config.is_empty() {
            None
        } else {
            Some(serde_json::Value::Object(config))
        }
    }
}

fn chrono_today() -> String {
    // Simple date without pulling in chrono: use system time
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let days = now / 86400;
    // Convert days since epoch to YYYY-MM-DD
    epoch_days_to_date(days)
}

fn subtract_days(today: &str, days: u64) -> Option<String> {
    // Parse YYYY-MM-DD back to epoch days, subtract, convert back
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let target = now.saturating_sub(days * 86400);
    let target_days = target / 86400;
    let _ = today; // we recalculate from epoch
    Some(epoch_days_to_date(target_days))
}

fn epoch_days_to_date(total_days: u64) -> String {
    // Algorithm to convert days since 1970-01-01 to YYYY-MM-DD
    let z = total_days as i64 + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}")
}

#[derive(Deserialize)]
struct XaiResponse {
    output: Option<Vec<XaiOutputItem>>,
}

#[derive(Deserialize)]
struct XaiOutputItem {
    #[serde(rename = "type")]
    item_type: Option<String>,
    content: Option<Vec<XaiContent>>,
    text: Option<String>,
}

#[derive(Deserialize)]
struct XaiContent {
    #[serde(rename = "type")]
    content_type: Option<String>,
    text: Option<String>,
    url: Option<String>,
    annotations: Option<Vec<XaiAnnotation>>,
}

#[derive(Deserialize)]
struct XaiAnnotation {
    #[serde(rename = "type")]
    annotation_type: Option<String>,
    url: Option<String>,
}

fn extract_text(resp: &XaiResponse) -> String {
    let mut parts = Vec::new();
    if let Some(output) = &resp.output {
        for item in output {
            if item.item_type.as_deref() == Some("message") {
                if let Some(content) = &item.content {
                    for c in content {
                        if c.content_type.as_deref() == Some("output_text") {
                            if let Some(text) = &c.text {
                                parts.push(text.clone());
                            }
                        }
                    }
                }
            } else if let Some(text) = &item.text {
                parts.push(text.clone());
            }
        }
    }
    parts.join("\n")
}

fn extract_citations(resp: &XaiResponse) -> Vec<String> {
    let mut urls = Vec::new();
    let mut seen = std::collections::HashSet::new();
    if let Some(output) = &resp.output {
        for item in output {
            if let Some(content) = &item.content {
                for c in content {
                    // Extract from url_citation annotations (primary method)
                    if let Some(annotations) = &c.annotations {
                        for ann in annotations {
                            if ann.annotation_type.as_deref() == Some("url_citation") {
                                if let Some(url) = &ann.url {
                                    if seen.insert(url.clone()) {
                                        urls.push(url.clone());
                                    }
                                }
                            }
                        }
                    }
                    // Fallback: check for cite/url content types
                    if c.content_type.as_deref() == Some("cite") || c.content_type.as_deref() == Some("url") {
                        if let Some(url) = &c.url {
                            if seen.insert(url.clone()) {
                                urls.push(url.clone());
                            }
                        }
                    }
                }
            }
        }
    }
    urls
}

#[async_trait]
impl super::Provider for Xai {
    fn name(&self) -> &'static str {
        "xai"
    }
    fn env_keys(&self) -> &[&'static str] { &["XAI_API_KEY", "SEARCH_KEYS_XAI"] }
    fn capabilities(&self) -> &[&'static str] {
        &["social"]
    }
    fn is_configured(&self) -> bool {
        !self.api_key().is_empty()
    }
    fn timeout(&self) -> Duration {
        Duration::from_secs(60)
    }

    async fn search(
        &self,
        query: &str,
        count: usize,
        opts: &SearchOpts,
    ) -> Result<Vec<SearchResult>, SearchError> {
        let prompt = format!(
            "Search X (Twitter) for posts about: {query}\n\
             Return up to {count} recent and relevant posts.\n\
             For each post include: author @username, post text, date/time, \
             and engagement metrics (likes, reposts, replies) if available.\n\
             Format as a clear summary."
        );

        let x_config = self.build_x_search_config(opts);
        let resp = self.call_responses_api(&prompt, x_config).await?;

        let mut results = Vec::new();
        let answer = extract_text(&resp);

        if !answer.is_empty() {
            results.push(SearchResult {
                title: "X Summary".to_string(),
                url: format!("https://x.com/search?q={}", query.replace(' ', "+")),
                snippet: answer,
                source: "xai".to_string(),
                published: None,
                image_url: None,
                extra: None,
            });
        }

        // Append citations as separate results
        let citations = extract_citations(&resp);
        for cite_url in citations {
            let hostname = url::Url::parse(&cite_url)
                .ok()
                .and_then(|u| u.path().split('/').nth(1).map(|s| format!("@{s}")))
                .unwrap_or_else(|| cite_url.clone());

            results.push(SearchResult {
                title: hostname,
                url: cite_url,
                snippet: "[Citation from X]".to_string(),
                source: "xai_citation".to_string(),
                published: None,
                image_url: None,
                extra: None,
            });
        }

        Ok(results)
    }

    async fn search_news(
        &self,
        query: &str,
        count: usize,
        opts: &SearchOpts,
    ) -> Result<Vec<SearchResult>, SearchError> {
        // X is inherently news-like; delegate to search
        self.search(query, count, opts).await
    }
}
