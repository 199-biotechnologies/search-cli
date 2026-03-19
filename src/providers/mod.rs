pub mod brave;
pub mod exa;
pub mod firecrawl;
pub mod jina;
pub mod serper;

use crate::context::AppContext;
use crate::errors::SearchError;
use crate::types::SearchResult;
use async_trait::async_trait;
use std::sync::Arc;
use std::time::Duration;

#[async_trait]
pub trait Provider: Send + Sync {
    fn name(&self) -> &'static str;
    fn capabilities(&self) -> &[&'static str];
    fn is_configured(&self) -> bool;
    fn timeout(&self) -> Duration {
        Duration::from_secs(10)
    }

    async fn search(&self, query: &str, count: usize) -> Result<Vec<SearchResult>, SearchError>;
    async fn search_news(&self, query: &str, count: usize)
        -> Result<Vec<SearchResult>, SearchError>;
}

pub fn build_providers(ctx: &Arc<AppContext>) -> Vec<Box<dyn Provider>> {
    vec![
        Box::new(brave::Brave::new(ctx.clone())),
        Box::new(serper::Serper::new(ctx.clone())),
        Box::new(exa::Exa::new(ctx.clone())),
        Box::new(jina::Jina::new(ctx.clone())),
        Box::new(firecrawl::Firecrawl::new(ctx.clone())),
    ]
}
