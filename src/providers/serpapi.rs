use crate::context::AppContext;
use crate::errors::SearchError;
use crate::types::{SearchOpts, SearchResult};
use async_trait::async_trait;
use std::sync::Arc;
use std::time::Duration;

pub struct SerpApi {
    ctx: Arc<AppContext>,
}

impl SerpApi {
    pub fn new(ctx: Arc<AppContext>) -> Self {
        Self { ctx }
    }

    fn api_key(&self) -> &str {
        &self.ctx.config.keys.serpapi
    }

    pub fn is_configured(&self) -> bool {
        !self.api_key().is_empty()
    }

    async fn query_endpoint(
        &self,
        engine: &str,
        query: &str,
        count: usize,
        opts: &SearchOpts,
    ) -> Result<serde_json::Value, SearchError> {
        if self.api_key().is_empty() {
            return Err(SearchError::AuthMissing { provider: "serpapi" });
        }

        let q = augment_query(query, opts);
        let client = &self.ctx.client;
        let api_key = self.api_key().to_string();

        let mut params = vec![
            ("engine".to_string(), engine.to_string()),
            ("q".to_string(), q),
            ("num".to_string(), count.to_string()),
            ("api_key".to_string(), api_key),
        ];

        if let Some(f) = &opts.freshness {
            let tbs = match f.as_str() {
                "day" => "qdr:d",
                "week" => "qdr:w",
                "month" => "qdr:m",
                "year" => "qdr:y",
                other => other,
            };
            params.push(("tbs".to_string(), tbs.to_string()));
        }

        super::retry_request(|| async {
            let resp = client
                .get("https://serpapi.com/search")
                .query(&params)
                .send()
                .await?;

            if resp.status() == 429 {
                return Err(SearchError::RateLimited { provider: "serpapi" });
            }
            if !resp.status().is_success() {
                return Err(SearchError::Api {
                    provider: "serpapi",
                    code: "api_error",
                    message: format!("HTTP {}", resp.status()),
                });
            }

            Ok(resp.json().await?)
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

fn parse_results(body: &serde_json::Value, key: &str, source: &str) -> Vec<SearchResult> {
    let items = body.get(key).and_then(|v| v.as_array());
    items
        .map(|arr| {
            arr.iter()
                .map(|item| {
                    let title = item
                        .get("title")
                        .and_then(|v| v.as_str())
                        .unwrap_or_default()
                        .to_string();
                    let url = item
                        .get("link")
                        .and_then(|v| v.as_str())
                        .unwrap_or_default()
                        .to_string();
                    let snippet = item
                        .get("snippet")
                        .and_then(|v| v.as_str())
                        .unwrap_or_default()
                        .to_string();
                    let published = item.get("date").and_then(|v| v.as_str()).map(String::from);
                    let image_url = None;

                    // For scholar results, attach citation info as extra
                    let extra = if source == "serpapi_scholar" {
                        let mut map = serde_json::Map::new();
                        if let Some(pub_info) = item.get("publication_info") {
                            map.insert("publication_info".to_string(), pub_info.clone());
                        }
                        if let Some(cited) = item.get("cited_by") {
                            map.insert("cited_by".to_string(), cited.clone());
                        }
                        if map.is_empty() {
                            None
                        } else {
                            Some(serde_json::Value::Object(map))
                        }
                    } else {
                        None
                    };

                    SearchResult {
                        title,
                        url,
                        snippet,
                        source: source.to_string(),
                        published,
                        image_url,
                        extra,
                    }
                })
                .collect()
        })
        .unwrap_or_default()
}

#[async_trait]
impl super::Provider for SerpApi {
    fn name(&self) -> &'static str {
        "serpapi"
    }
    fn capabilities(&self) -> &[&'static str] {
        &["general", "news", "scholar", "images"]
    }
    fn is_configured(&self) -> bool {
        !self.api_key().is_empty()
    }
    fn timeout(&self) -> Duration {
        Duration::from_secs(10)
    }

    async fn search(
        &self,
        query: &str,
        count: usize,
        opts: &SearchOpts,
    ) -> Result<Vec<SearchResult>, SearchError> {
        let body = self.query_endpoint("google", query, count, opts).await?;
        Ok(parse_results(&body, "organic_results", "serpapi"))
    }

    async fn search_news(
        &self,
        query: &str,
        count: usize,
        opts: &SearchOpts,
    ) -> Result<Vec<SearchResult>, SearchError> {
        let body = self.query_endpoint("google_news", query, count, opts).await?;
        Ok(parse_results(&body, "news_results", "serpapi_news"))
    }
}

impl SerpApi {
    pub async fn search_scholar(
        &self,
        query: &str,
        count: usize,
    ) -> Result<Vec<SearchResult>, SearchError> {
        let body = self
            .query_endpoint("google_scholar", query, count, &SearchOpts::default())
            .await?;
        Ok(parse_results(&body, "organic_results", "serpapi_scholar"))
    }
}
