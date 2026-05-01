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
pub mod you;

use crate::context::AppContext;
use crate::errors::SearchError;
use crate::types::{SearchOpts, SearchResult};
use async_trait::async_trait;
use backon::{ExponentialBuilder, Retryable};
use tl::ParserOptions;
use std::sync::Arc;
use std::time::Duration;

/// Append `site:` / `-site:` domain filters to a query string.
/// Shared by brave, serper, and you providers.
pub fn augment_query(query: &str, opts: &SearchOpts) -> String {
    let mut q = query.to_string();
    for d in &opts.include_domains {
        let sanitized = sanitize_domain(d);
        q.push_str(&format!(" site:{}", sanitized));
    }
    for d in &opts.exclude_domains {
        let sanitized = sanitize_domain(d);
        q.push_str(&format!(" -site:{}", sanitized));
    }
    q
}

/// Sanitize a domain string before injecting into a search query.
/// Rejects CRLF, spaces, quotes, operators (OR, AND, NOT) to prevent injection.
fn sanitize_domain(domain: &str) -> String {
    let forbidden = ['\r', '\n', '"', '\'', ' ', '(', ')', ';'];
    if domain.chars().any(|c| forbidden.contains(&c))
        || domain.contains(" OR ")
        || domain.contains(" AND ")
        || domain.contains(" NOT ")
    {
        tracing::warn!(event = "invalid_domain_rejected", domain = %domain);
        return "invalid".to_string();
    }
    domain.trim().to_string()
}

/// Extract the `<title>` text from an HTML document.
/// Shared by stealth and browserless providers.
pub fn extract_title(html: &str) -> Option<String> {
    let dom = tl::parse(html, ParserOptions::default()).ok()?;
    let parser = dom.parser();
    let mut titles = dom.query_selector("title")?;
    let node = titles.next()?.get(parser)?;
    let text = node.inner_text(parser).trim().to_string();
    if text.is_empty() { None } else { Some(text) }
}

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
    .when(|e| matches!(e, SearchError::Http(_) | SearchError::Wreq(_) | SearchError::Api { code: "server_error", .. }))
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
        Box::new(you::You::new(ctx.clone())),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::SearchOpts;

    // Task 5: augment_query tests
    #[test]
    fn test_augment_query_empty_query_no_domains() {
        let opts = SearchOpts::default();
        let result = augment_query("", &opts);
        assert_eq!(result, "");
    }

    #[test]
    fn test_augment_query_query_no_domains() {
        let opts = SearchOpts::default();
        let result = augment_query("hello world", &opts);
        assert_eq!(result, "hello world");
    }

    #[test]
    fn test_augment_query_single_include() {
        let mut opts = SearchOpts::default();
        opts.include_domains = vec!["example.com".to_string()];
        let result = augment_query("hello", &opts);
        assert_eq!(result, "hello site:example.com");
    }

    #[test]
    fn test_augment_query_multiple_includes() {
        let mut opts = SearchOpts::default();
        opts.include_domains = vec!["example.com".to_string(), "test.org".to_string()];
        let result = augment_query("hello", &opts);
        assert_eq!(result, "hello site:example.com site:test.org");
    }

    #[test]
    fn test_augment_query_single_exclude() {
        let mut opts = SearchOpts::default();
        opts.exclude_domains = vec!["spam.com".to_string()];
        let result = augment_query("hello", &opts);
        assert_eq!(result, "hello -site:spam.com");
    }

    #[test]
    fn test_augment_query_multiple_excludes() {
        let mut opts = SearchOpts::default();
        opts.exclude_domains = vec!["spam.com".to_string(), "ads.net".to_string()];
        let result = augment_query("hello", &opts);
        assert_eq!(result, "hello -site:spam.com -site:ads.net");
    }

    #[test]
    fn test_augment_query_mixed() {
        let mut opts = SearchOpts::default();
        opts.include_domains = vec!["good.com".to_string()];
        opts.exclude_domains = vec!["bad.com".to_string()];
        let result = augment_query("hello", &opts);
        assert_eq!(result, "hello site:good.com -site:bad.com");
    }

    #[test]
    fn test_augment_query_preserves_spaces() {
        let opts = SearchOpts::default();
        let result = augment_query("hello  world  test", &opts);
        assert_eq!(result, "hello  world  test");
    }

    // Task 6: extract_title tests
    #[test]
    fn test_extract_title_valid() {
        let html = "<html><head><title>Hello World</title></head></html>";
        let result = extract_title(html);
        assert_eq!(result, Some("Hello World".to_string()));
    }

    #[test]
    fn test_extract_title_trims() {
        let html = "<html><head><title>  Hello World  </title></head></html>";
        let result = extract_title(html);
        assert_eq!(result, Some("Hello World".to_string()));
    }

    #[test]
    fn test_extract_title_empty() {
        let html = "<html><head><title></title></head></html>";
        let result = extract_title(html);
        assert_eq!(result, None);
    }

    #[test]
    fn test_extract_title_no_tag() {
        let html = "<html><body>No title here</body></html>";
        let result = extract_title(html);
        assert_eq!(result, None);
    }

    #[test]
    fn test_extract_title_multiple() {
        // tl parser should return the first title
        let html = "<html><head><title>First</title><title>Second</title></head></html>";
        let result = extract_title(html);
        assert_eq!(result, Some("First".to_string()));
    }

    #[test]
    fn test_extract_title_malformed() {
        let html = "<html><head><title>Unclosed";
        let result = extract_title(html);
        // Unclosed title tag — tl parser may or may not extract; accept both
        // but at least verify it doesn't panic
        assert!(result.is_none() || result == Some("Unclosed".to_string()));
    }

    #[tokio::test]
    async fn test_retry_request_retries_on_wreq_error() {
        use std::sync::{Arc, Mutex};
        let attempt_count = Arc::new(Mutex::new(0));

        // Create a wreq::Error from a serde_json::Error (which wreq::Error implements From for)
        // We'll recreate the error inside the closure since wreq::Error isn't Clone
        let result: Result<(), SearchError> = retry_request(|| {
            let count = attempt_count.clone();
            async move {
                let mut c = count.lock().unwrap();
                *c += 1;
                // Create a new wreq::Error each time (from a fresh serde_json::Error)
                let json_err = serde_json::from_str::<serde_json::Value>("invalid json").unwrap_err();
                let wreq_err = wreq::Error::from(json_err);
                Err(SearchError::Wreq(wreq_err))
            }
        })
        .await;

        // Verify the function was called 4 times (1 initial + 3 retries)
        let final_count = *attempt_count.lock().unwrap();
        assert_eq!(final_count, 4, "Expected 4 attempts (1 initial + 3 retries)");

        // Verify we get an error back
        assert!(result.is_err());
        match result {
            Err(SearchError::Wreq(_)) => (),
            _ => panic!("Expected SearchError::Wreq"),
        }
    }
}
