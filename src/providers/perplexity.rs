use crate::context::AppContext;
use crate::errors::SearchError;
use crate::types::{SearchOpts, SearchResult};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;
use std::sync::Arc;
use std::time::Duration;

pub struct Perplexity {
    ctx: Arc<AppContext>,
}

impl Perplexity {
    pub fn new(ctx: Arc<AppContext>) -> Self {
        Self { ctx }
    }

    fn api_key(&self) -> &str {
        &self.ctx.config.keys.perplexity
    }

    async fn do_search(
        &self,
        query: &str,
        opts: &SearchOpts,
        recency_filter: Option<&str>,
    ) -> Result<Vec<SearchResult>, SearchError> {
        if self.api_key().is_empty() {
            return Err(SearchError::AuthMissing {
                provider: "perplexity",
            });
        }

        let mut body = json!({
            "model": "sonar",
            "messages": [{"role": "user", "content": query}],
        });

        // Apply domain filter from opts
        if !opts.include_domains.is_empty() {
            body["search_domain_filter"] = json!(opts.include_domains);
        }

        // Apply recency filter: explicit param first, then opts.freshness
        let recency = recency_filter.or(opts.freshness.as_deref());
        if let Some(r) = recency {
            let rf = match r {
                "day" => "day",
                "week" => "week",
                "month" => "month",
                other => other,
            };
            body["search_recency_filter"] = json!(rf);
        }

        let client = &self.ctx.client;
        let key = self.api_key().to_string();
        let resp = super::retry_request(|| {
            let body = body.clone();
            let key = key.clone();
            async move {
                let r = client
                    .post("https://api.perplexity.ai/chat/completions")
                    .header("Authorization", format!("Bearer {key}"))
                    .json(&body)
                    .send()
                    .await?;
                if r.status() == 429 {
                    return Err(SearchError::RateLimited {
                        provider: "perplexity",
                    });
                }
                if !r.status().is_success() {
                    return Err(SearchError::Api {
                        provider: "perplexity",
                        code: "api_error",
                        message: format!("HTTP {}", r.status()),
                    });
                }
                Ok(r.json::<PerplexityResponse>().await?)
            }
        })
        .await?;

        let mut results = Vec::new();

        // Extract the AI answer from the first choice
        let answer = resp
            .choices
            .first()
            .map(|c| c.message.content.clone())
            .unwrap_or_default();

        results.push(SearchResult {
            title: "AI Answer".to_string(),
            url: "perplexity://answer".to_string(),
            snippet: answer,
            source: "perplexity".to_string(),
            published: None,
            image_url: None,
            extra: None,
        });

        // Add one result per citation URL
        if let Some(citations) = resp.citations {
            for cite_url in citations {
                let hostname = url::Url::parse(&cite_url)
                    .ok()
                    .and_then(|u| u.host_str().map(|h| h.to_string()))
                    .unwrap_or_else(|| cite_url.clone());

                results.push(SearchResult {
                    title: hostname,
                    url: cite_url,
                    snippet: "[Citation]".to_string(),
                    source: "perplexity_citation".to_string(),
                    published: None,
                    image_url: None,
                    extra: None,
                });
            }
        }

        Ok(results)
    }
}

#[derive(Deserialize)]
struct PerplexityResponse {
    choices: Vec<PerplexityChoice>,
    citations: Option<Vec<String>>,
}

#[derive(Deserialize)]
struct PerplexityChoice {
    message: PerplexityMessage,
}

#[derive(Deserialize)]
struct PerplexityMessage {
    content: String,
}

#[async_trait]
impl super::Provider for Perplexity {
    fn name(&self) -> &'static str {
        "perplexity"
    }
    fn capabilities(&self) -> &[&'static str] {
        &["general", "news", "academic", "deep"]
    }
    fn is_configured(&self) -> bool {
        !self.api_key().is_empty()
    }
    fn timeout(&self) -> Duration {
        Duration::from_secs(20)
    }

    async fn search(
        &self,
        query: &str,
        _count: usize,
        opts: &SearchOpts,
    ) -> Result<Vec<SearchResult>, SearchError> {
        self.do_search(query, opts, None).await
    }

    async fn search_news(
        &self,
        query: &str,
        _count: usize,
        opts: &SearchOpts,
    ) -> Result<Vec<SearchResult>, SearchError> {
        // Default to "day" recency for news if no freshness specified
        let recency = opts.freshness.as_deref().unwrap_or("day");
        self.do_search(query, opts, Some(recency)).await
    }
}
