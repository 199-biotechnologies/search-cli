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
        model: &str,
        recency_filter: Option<&str>,
        search_mode: Option<&str>,
    ) -> Result<Vec<SearchResult>, SearchError> {
        if self.api_key().is_empty() {
            return Err(SearchError::AuthMissing {
                provider: "perplexity",
            });
        }

        let mut body = json!({
            "model": model,
            "messages": [{"role": "user", "content": query}],
            "return_related_questions": false,
            "return_images": false,
            "web_search_options": {
                "search_context_size": "high"
            },
        });

        // Search mode: academic, sec, or web (default)
        if let Some(sm) = search_mode {
            body["search_mode"] = json!(sm);
        }

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
        let source_label = format!("perplexity_{model}");

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
            source: source_label.clone(),
            published: None,
            image_url: None,
            extra: None,
        });

        // Use structured search_results if available (sonar-pro returns these)
        if let Some(search_results) = resp.search_results {
            for sr in search_results {
                results.push(SearchResult {
                    title: sr.title.unwrap_or_default(),
                    url: sr.url.unwrap_or_default(),
                    snippet: sr.snippet.unwrap_or_else(|| "[Search result]".to_string()),
                    source: format!("{source_label}_result"),
                    published: sr.date,
                    image_url: None,
                    extra: None,
                });
            }
        } else if let Some(citations) = resp.citations {
            // Fallback: one result per citation URL
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
    search_results: Option<Vec<PerplexitySearchResult>>,
}

#[derive(Deserialize)]
struct PerplexityChoice {
    message: PerplexityMessage,
}

#[derive(Deserialize)]
struct PerplexityMessage {
    content: String,
}

#[derive(Deserialize)]
struct PerplexitySearchResult {
    title: Option<String>,
    url: Option<String>,
    snippet: Option<String>,
    date: Option<String>,
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
        Duration::from_secs(30)
    }

    async fn search(
        &self,
        query: &str,
        _count: usize,
        opts: &SearchOpts,
    ) -> Result<Vec<SearchResult>, SearchError> {
        // Use sonar-pro for better results with structured search_results
        self.do_search(query, opts, "sonar-pro", None, None).await
    }

    async fn search_news(
        &self,
        query: &str,
        _count: usize,
        opts: &SearchOpts,
    ) -> Result<Vec<SearchResult>, SearchError> {
        // Default to "day" recency for news if no freshness specified
        let recency = opts.freshness.as_deref().unwrap_or("day");
        self.do_search(query, opts, "sonar-pro", Some(recency), None)
            .await
    }
}

impl Perplexity {
    /// Academic search using search_mode: "academic"
    pub async fn search_academic(
        &self,
        query: &str,
        count: usize,
        opts: &SearchOpts,
    ) -> Result<Vec<SearchResult>, SearchError> {
        self.do_search(query, opts, "sonar-pro", None, Some("academic"))
            .await
    }

    /// Deep research using sonar-reasoning-pro for complex queries
    pub async fn search_deep(
        &self,
        query: &str,
        count: usize,
        opts: &SearchOpts,
    ) -> Result<Vec<SearchResult>, SearchError> {
        self.do_search(query, opts, "sonar-reasoning-pro", None, None)
            .await
    }
}
