use crate::context::AppContext;
use crate::errors::SearchError;
use crate::types::{SearchOpts, SearchResult};
use async_trait::async_trait;
use serde::Deserialize;
use std::sync::Arc;

pub struct Jina {
    ctx: Arc<AppContext>,
}

impl Jina {
    pub fn new(ctx: Arc<AppContext>) -> Self {
        Self { ctx }
    }

    fn api_key(&self) -> String {
        super::resolve_key(&self.ctx.config.keys.jina, "JINA_API_KEY")
    }

    fn classify_rejection(body_text: &str) -> Option<SearchError> {
        let lower = body_text.to_lowercase();
        if body_text.contains("1010") || lower.contains("cloudflare") {
            return Some(SearchError::Api {
                provider: "jina",
                code: "cloudflare_1010",
                message: "Jina request blocked by Cloudflare (1010)".to_string(),
            });
        }
        None
    }
}

#[derive(Deserialize)]
struct JinaSearchResponse {
    data: Option<Vec<JinaResult>>,
}

#[derive(Deserialize)]
struct JinaResult {
    title: Option<String>,
    url: Option<String>,
    description: Option<String>,
    content: Option<String>,
}

#[async_trait]
impl super::Provider for Jina {
    fn name(&self) -> &'static str {
        "jina"
    }

    fn env_keys(&self) -> &[&'static str] { &["JINA_API_KEY", "SEARCH_KEYS_JINA"] }
    fn capabilities(&self) -> &[&'static str] {
        &["general", "extract"]
    }

    fn is_configured(&self) -> bool {
        !self.api_key().is_empty()
    }


    async fn search(&self, query: &str, count: usize, opts: &SearchOpts) -> Result<Vec<SearchResult>, SearchError> {
        if !self.is_configured() {
            return Err(SearchError::AuthMissing { provider: "jina" });
        }

        let client = &self.ctx.client;
        let auth = format!("Bearer {}", self.api_key());
        let count_str = count.to_string();

        // Apply domain filtering via query augmentation (Jina API doesn't have native domain filters)
        let q = if opts.include_domains.is_empty() && opts.exclude_domains.is_empty() {
            query.to_string()
        } else {
            let mut q = query.to_string();
            for d in &opts.include_domains {
                q = format!("{q} site:{d}");
            }
            for d in &opts.exclude_domains {
                q = format!("{q} -site:{d}");
            }
            q
        };

        super::retry_request(|| async {
            let resp = client
                .get("https://s.jina.ai/")
                .header("Authorization", &auth)
                .header("Accept", "application/json")
                .header("X-Retain-Images", "none")
                .query(&[("q", q.as_str()), ("count", &count_str)])
                .send()
                .await?;

            if resp.status() == 429 {
                return Err(SearchError::RateLimited { provider: "jina" });
            }
            if !resp.status().is_success() {
                let status = resp.status();
                let body_text = resp.text().await.unwrap_or_default();

                if let Some(classified) = Self::classify_rejection(&body_text) {
                    return Err(classified);
                }

                return Err(SearchError::Api {
                    provider: "jina",
                    code: "api_error",
                    message: format!("HTTP {}", status),
                });
            }

            let body: JinaSearchResponse = resp.json().await?;
            let results = body
                .data
                .unwrap_or_default()
                .into_iter()
                .map(|r| SearchResult {
                    title: r.title.unwrap_or_default(),
                    url: r.url.unwrap_or_default(),
                    snippet: r.description.or(r.content).unwrap_or_default(),
                    source: "jina".to_string(),
                    published: None,
                    image_url: None,
                    extra: None,
                })
                .collect();

            Ok(results)
        })
        .await
    }

    async fn search_news(
        &self,
        _query: &str,
        _count: usize,
        _opts: &SearchOpts,
    ) -> Result<Vec<SearchResult>, SearchError> {
        Ok(vec![]) // Jina doesn't have a dedicated news endpoint
    }
}

impl Jina {
    pub async fn read_url(&self, url: &str) -> Result<Vec<SearchResult>, SearchError> {
        if self.api_key().is_empty() {
            return Err(SearchError::AuthMissing { provider: "jina" });
        }

        let reader_url = format!("https://r.jina.ai/{url}");
        let client = &self.ctx.client;
        let auth = format!("Bearer {}", self.api_key());

        super::retry_request(|| async {
            let resp = client
                .get(&reader_url)
                .header("Authorization", &auth)
                .header("Accept", "application/json")
                .header("X-Retain-Images", "none")
                .send()
                .await?;

            if !resp.status().is_success() {
                let status = resp.status();
                let body_text = resp.text().await.unwrap_or_default();

                if let Some(classified) = Self::classify_rejection(&body_text) {
                    return Err(classified);
                }

                return Err(SearchError::Api {
                    provider: "jina",
                    code: "api_error",
                    message: format!("HTTP {}", status),
                });
            }

            let body: serde_json::Value = resp.json().await?;
            let data = body.get("data");
            let title = data
                .and_then(|d| d.get("title"))
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            let content = data
                .and_then(|d| d.get("content"))
                .and_then(|v| v.as_str())
                .unwrap_or_default();

            Ok(vec![SearchResult {
                title: title.to_string(),
                url: url.to_string(),
                snippet: content.to_string(),
                source: "jina_reader".to_string(),
                published: None,
                image_url: None,
                extra: None,
            }])
        })
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_classify_rejection_cloudflare_1010() {
        let err = Jina::classify_rejection("Error 1010: Cloudflare access denied")
            .expect("expected classified rejection");
        match err {
            SearchError::Api { code, .. } => assert_eq!(code, "cloudflare_1010"),
            _ => panic!("expected SearchError::Api"),
        }
    }

    #[test]
    fn test_classify_rejection_none_for_unrelated_text() {
        assert!(Jina::classify_rejection("plain api error").is_none());
    }
}
