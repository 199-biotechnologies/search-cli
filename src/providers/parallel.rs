use crate::context::AppContext;
use crate::errors::SearchError;
use crate::types::{SearchOpts, SearchResult};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;
use std::sync::Arc;

pub struct Parallel {
    ctx: Arc<AppContext>,
}

impl Parallel {
    pub fn new(ctx: Arc<AppContext>) -> Self {
        Self { ctx }
    }

    fn api_key(&self) -> String {
        super::resolve_key(&self.ctx.config.keys.parallel, "PARALLEL_API_KEY")
    }
}

#[derive(Deserialize)]
struct ParallelResponse {
    results: Option<Vec<ParallelResult>>,
}

#[derive(Deserialize)]
struct ParallelResult {
    title: Option<String>,
    url: Option<String>,
    excerpts: Option<Vec<String>>,
}

#[async_trait]
impl super::Provider for Parallel {
    fn name(&self) -> &'static str {
        "parallel"
    }

    fn capabilities(&self) -> &[&'static str] {
        &["general", "news", "deep"]
    }

    fn env_keys(&self) -> &[&'static str] {
        &["PARALLEL_API_KEY", "SEARCH_KEYS_PARALLEL"]
    }

    fn is_configured(&self) -> bool {
        !self.api_key().is_empty()
    }

    async fn search(
        &self,
        query: &str,
        count: usize,
        _opts: &SearchOpts,
    ) -> Result<Vec<SearchResult>, SearchError> {
        if !self.is_configured() {
            return Err(SearchError::AuthMissing {
                provider: "parallel",
            });
        }

        let client = &self.ctx.client;
        let api_key = self.api_key();

        super::retry_request(|| async {
            let body = json!({
                "objective": query,
                "search_queries": [query],
                "mode": "fast",
                "num_results": count,
                "excerpts": { "max_chars_per_result": 3000 },
            });

            let resp = client
                .post("https://api.parallel.ai/v1beta/search")
                .header("x-api-key", api_key.as_str())
                .header("Content-Type", "application/json")
                .json(&body)
                .send()
                .await?;

            let status = resp.status();
            if status == 429 {
                return Err(SearchError::RateLimited {
                    provider: "parallel",
                });
            }
            if !status.is_success() {
                return Err(SearchError::Api {
                    provider: "parallel",
                    code: "api_error",
                    message: format!("Parallel API error ({})", status),
                });
            }

            let data: ParallelResponse = resp.json().await?;
            let results = data
                .results
                .unwrap_or_default()
                .into_iter()
                .filter_map(|r| {
                    let url = r.url?;
                    Some(SearchResult {
                        title: r.title.unwrap_or_default(),
                        url,
                        snippet: r.excerpts.unwrap_or_default().join("\n\n"),
                        source: "parallel".to_string(),
                        published: None,
                        image_url: None,
                        extra: None,
                    })
                })
                .collect();

            Ok(results)
        })
        .await
    }

    async fn search_news(
        &self,
        query: &str,
        count: usize,
        opts: &SearchOpts,
    ) -> Result<Vec<SearchResult>, SearchError> {
        self.search(query, count, opts).await
    }
}
