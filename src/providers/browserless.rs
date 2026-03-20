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

    fn api_key(&self) -> &str {
        &self.ctx.config.keys.browserless
    }

    /// Scrape a URL using Browserless cloud browser (handles Cloudflare, JS rendering)
    pub async fn scrape_url(&self, url: &str) -> Result<Vec<SearchResult>, SearchError> {
        if self.api_key().is_empty() {
            return Err(SearchError::AuthMissing {
                provider: "browserless",
            });
        }

        let endpoint = format!(
            "https://production-sfo.browserless.io/content?token={}",
            self.api_key()
        );

        let body = serde_json::json!({
            "url": url,
            "waitForSelector": { "selector": "body", "timeout": 10000 }
        });

        let client = &self.ctx.client;

        let resp = super::retry_request(|| async {
            let r = client
                .post(&endpoint)
                .header("Content-Type", "application/json")
                .json(&body)
                .send()
                .await?;

            if r.status() == 429 {
                return Err(SearchError::RateLimited {
                    provider: "browserless",
                });
            }
            if !r.status().is_success() {
                return Err(SearchError::Api {
                    provider: "browserless",
                    code: "api_error",
                    message: format!("HTTP {}", r.status()),
                });
            }

            Ok(r.text().await?)
        })
        .await?;

        // resp is fully rendered HTML — extract with readability
        let parsed_url = url::Url::parse(url)
            .unwrap_or_else(|_| url::Url::parse("https://example.com").unwrap());

        let mut cursor = std::io::Cursor::new(resp.as_bytes());
        let (title, text) = match readability::extractor::extract(&mut cursor, &parsed_url) {
            Ok(article) if !article.text.trim().is_empty() => {
                let title = if article.title.is_empty() {
                    url.to_string()
                } else {
                    article.title
                };
                (title, article.text)
            }
            _ => (url.to_string(), extract_text_simple(&resp)),
        };

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
