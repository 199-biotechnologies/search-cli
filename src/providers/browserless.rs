use crate::context::AppContext;
use crate::errors::SearchError;
use crate::types::{SearchOpts, SearchResult};
use async_trait::async_trait;
use std::sync::Arc;
use std::time::Duration;

pub struct Browserless {
    ctx: Arc<AppContext>,
}

impl Browserless {
    pub fn new(ctx: Arc<AppContext>) -> Self {
        Self { ctx }
    }

    fn api_key(&self) -> String {
        super::resolve_key(&self.ctx.config.keys.browserless, "BROWSERLESS_API_KEY")
    }

    fn classify_rejection(status: reqwest::StatusCode, body_text: &str) -> Option<SearchError> {
        // Browserless signature for key/endpoint auth mode mismatch.
        let lower = body_text.to_lowercase();
        let mismatch = lower.contains("auth")
            && lower.contains("mode")
            && lower.contains("mismatch");
        if status == reqwest::StatusCode::INTERNAL_SERVER_ERROR && mismatch {
            return Some(SearchError::Api {
                provider: "browserless",
                code: "auth_mode_mismatch",
                message: "Browserless auth mode mismatch between key and endpoint".to_string(),
            });
        }
        None
    }

    /// Scrape a URL using Browserless cloud browser (handles Cloudflare, JS rendering)
    pub async fn scrape_url(&self, url: &str) -> Result<Vec<SearchResult>, SearchError> {
        if self.api_key().is_empty() {
            return Err(SearchError::AuthMissing {
                provider: "browserless",
            });
        }

        let endpoint = "https://production-sfo.browserless.io/content";

        let body = serde_json::json!({
            "url": url,
            "waitForSelector": { "selector": "body", "timeout": 10000 }
        });

        let client = &self.ctx.client;
        let token = self.api_key().to_string();

        let resp = super::retry_request(|| async {
            let r = client
                .post(endpoint)
                .header("Content-Type", "application/json")
                .header("Authorization", format!("Bearer {}", token))
                .json(&body)
                .send()
                .await?;

            if r.status() == 429 {
                return Err(SearchError::RateLimited {
                    provider: "browserless",
                });
            }
            if !r.status().is_success() {
                let status = r.status();
                let body_text = r.text().await.unwrap_or_default();

                if let Some(classified) = Self::classify_rejection(status, &body_text) {
                    return Err(classified);
                }

                return Err(SearchError::Api {
                    provider: "browserless",
                    code: "api_error",
                    message: format!("HTTP {}", status),
                });
            }

            Ok(r.text().await?)
        })
        .await?;

    // Offload extraction to blocking pool so heavy HTML parsing doesn't block
    // the async runtime worker. Uses tl-based extraction (no readability/reqwest).
    let resp_for_extract = resp;
    let fallback_title = url.to_string();
    let (title, text) = tokio::task::spawn_blocking(move || {
        let title = extract_title(&resp_for_extract).unwrap_or_else(|| fallback_title.clone());
        let body = extract_text_simple(&resp_for_extract);
        (title, body)
    })
    .await
    .map_err(|e| SearchError::Api {
        provider: "browserless",
        code: "extraction_error",
        message: format!("Browserless extraction task failed: {e}"),
    })?;

        if text.trim().is_empty() {
            return Err(SearchError::Api {
                provider: "browserless",
                code: "extraction_error",
                message: "Page returned no extractable content".to_string(),
            });
        }

        Ok(vec![SearchResult {
            title,
            url: url.to_string(),
            snippet: text,
            source: "browserless".to_string(),
            published: None,
            image_url: None,
            extra: None,
        }])
    }
}

/// Extract <title> from HTML using tl parser
fn extract_title(html: &str) -> Option<String> {
    let dom = tl::parse(html, tl::ParserOptions::default()).ok()?;
    let parser = dom.parser();
    let mut titles = dom.query_selector("title")?;
    let node = titles.next()?.get(parser)?;
    let text = node.inner_text(parser).trim().to_string();
    if text.is_empty() { None } else { Some(text) }
}

/// Simple HTML tag stripper
fn extract_text_simple(html: &str) -> String {
    let mut text = String::with_capacity(html.len() / 3);
    let mut in_tag = false;
    for c in html.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => text.push(c),
            _ => {}
        }
    }
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[async_trait]
impl super::Provider for Browserless {
    fn name(&self) -> &'static str {
        "browserless"
    }

    fn env_keys(&self) -> &[&'static str] { &["BROWSERLESS_API_KEY", "SEARCH_KEYS_BROWSERLESS"] }
    fn capabilities(&self) -> &[&'static str] {
        &["scrape", "extract"]
    }

    fn is_configured(&self) -> bool {
        !self.api_key().is_empty()
    }

    fn timeout(&self) -> Duration {
        Duration::from_secs(30)
    }

    async fn search(
        &self,
        _query: &str,
        _count: usize,
        _opts: &SearchOpts,
    ) -> Result<Vec<SearchResult>, SearchError> {
        Ok(vec![])
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_classify_rejection_auth_mode_mismatch() {
        let err = Browserless::classify_rejection(
            reqwest::StatusCode::INTERNAL_SERVER_ERROR,
            "Auth mode mismatch detected between token and endpoint",
        )
        .expect("expected classified rejection");

        match err {
            SearchError::Api { code, .. } => assert_eq!(code, "auth_mode_mismatch"),
            _ => panic!("expected SearchError::Api"),
        }
    }

    #[test]
    fn test_classify_rejection_none_for_other_errors() {
        let err = Browserless::classify_rejection(
            reqwest::StatusCode::INTERNAL_SERVER_ERROR,
            "generic server failure",
        );
        assert!(err.is_none());
    }
}
