pub mod brave;
pub mod browserless;
pub mod exa;
pub mod firecrawl;
pub mod jina;
pub mod parallel;
pub mod perplexity;
pub mod serpapi;
pub mod serper;
pub mod stealth;
pub mod tavily;
pub mod xai;

use crate::context::AppContext;
use crate::errors::SearchError;
use crate::types::{SearchOpts, SearchResult};
use async_trait::async_trait;
use backon::{ExponentialBuilder, Retryable};
use std::sync::Arc;
use std::time::Duration;

pub async fn retry_request<F, Fut, T>(f: F) -> Result<T, SearchError>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<T, SearchError>>,
{
    let mut attempt = 0;
    f.retry(
        ExponentialBuilder::default()
        .with_min_delay(Duration::from_secs(1))
        .with_max_delay(Duration::from_secs(4))
        .with_max_times(3),
    )
    .notify(|e: &SearchError, dur| {
            attempt += 1;
            tracing::info!(
                event = "provider_retry",
                attempt = attempt,
                delay_ms = dur.as_millis() as u64,
                reason_code = e.error_code(),
                message = %e
            );
        })
    .when(|e| matches!(e, SearchError::Http(_)))
    .await
}

/// Check config key first, then fall back to standard env var.
pub fn resolve_key(config_value: &str, env_var: &str) -> String {
    if !config_value.is_empty() {
        return config_value.to_string();
    }
    std::env::var(env_var).unwrap_or_default()
}

#[async_trait]
pub trait Provider: Send + Sync {
    fn name(&self) -> &'static str;
    fn capabilities(&self) -> &[&'static str];
    fn is_configured(&self) -> bool;
    /// Standard env var names accepted by this provider (e.g. BRAVE_API_KEY).
    fn env_keys(&self) -> &[&'static str];
    fn timeout(&self) -> Duration {
        Duration::from_secs(10)
    }

    async fn search(&self, query: &str, count: usize, opts: &SearchOpts) -> Result<Vec<SearchResult>, SearchError>;
    async fn search_news(&self, query: &str, count: usize, opts: &SearchOpts)
        -> Result<Vec<SearchResult>, SearchError>;
}

pub fn build_providers(ctx: &Arc<AppContext>) -> Vec<Box<dyn Provider>> {
    vec![
        Box::new(parallel::Parallel::new(ctx.clone())),
        Box::new(brave::Brave::new(ctx.clone())),
        Box::new(serper::Serper::new(ctx.clone())),
        Box::new(exa::Exa::new(ctx.clone())),
        Box::new(jina::Jina::new(ctx.clone())),
        Box::new(stealth::Stealth::new(ctx.clone())),
        Box::new(firecrawl::Firecrawl::new(ctx.clone())),
        Box::new(tavily::Tavily::new(ctx.clone())),
        Box::new(browserless::Browserless::new(ctx.clone())),
        Box::new(perplexity::Perplexity::new(ctx.clone())),
        Box::new(serpapi::SerpApi::new(ctx.clone())),
        Box::new(xai::Xai::new(ctx.clone())),
    ]
}
