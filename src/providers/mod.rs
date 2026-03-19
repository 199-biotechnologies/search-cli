pub mod brave;
pub mod exa;
pub mod firecrawl;
pub mod jina;
pub mod perplexity;
pub mod serpapi;
pub mod serper;
pub mod tavily;

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
    f.retry(
        ExponentialBuilder::default()
            .with_min_delay(Duration::from_secs(1))
            .with_max_delay(Duration::from_secs(4))
            .with_max_times(3),
    )
    .when(|e| matches!(e, SearchError::Http(_)))
    .await
}

#[async_trait]
pub trait Provider: Send + Sync {
    fn name(&self) -> &'static str;
    fn capabilities(&self) -> &[&'static str];
    fn is_configured(&self) -> bool;
    fn timeout(&self) -> Duration {
        Duration::from_secs(10)
    }

    async fn search(&self, query: &str, count: usize, opts: &SearchOpts) -> Result<Vec<SearchResult>, SearchError>;
    async fn search_news(&self, query: &str, count: usize, opts: &SearchOpts)
        -> Result<Vec<SearchResult>, SearchError>;
}

pub fn build_providers(ctx: &Arc<AppContext>) -> Vec<Box<dyn Provider>> {
    vec![
        Box::new(brave::Brave::new(ctx.clone())),
        Box::new(serper::Serper::new(ctx.clone())),
        Box::new(exa::Exa::new(ctx.clone())),
        Box::new(jina::Jina::new(ctx.clone())),
        Box::new(firecrawl::Firecrawl::new(ctx.clone())),
        Box::new(tavily::Tavily::new(ctx.clone())),
        Box::new(perplexity::Perplexity::new(ctx.clone())),
        Box::new(serpapi::SerpApi::new(ctx.clone())),
    ]
}
