use crate::classify::classify_intent;
use crate::context::AppContext;
use crate::errors::SearchError;
use crate::providers::{self, Provider};
use crate::types::{Mode, ProviderFailureDetail, ResponseMetadata, SearchOpts, SearchResult, SearchResponse};
use std::collections::HashSet;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::task::JoinSet;
use tokio::time::timeout;

/// Which providers to query for each mode
fn providers_for_mode(mode: Mode) -> &'static [&'static str] {
    match mode {
        Mode::Auto | Mode::General => &["parallel", "brave", "serper", "exa", "jina", "tavily", "perplexity", "you"],
        Mode::News => &["parallel", "brave", "serper", "tavily", "perplexity", "you"],
        Mode::Academic => &["exa", "serper", "tavily", "perplexity"],
        Mode::Deep => &["parallel", "brave", "exa", "serper", "tavily", "perplexity", "xai", "you"],
        Mode::Scholar => &["serper", "serpapi"],
        Mode::Patents => &["serper"],
        Mode::People => &["exa"],
        Mode::Images => &["serper"],
        Mode::Places => &["serper"],
        Mode::Extract | Mode::Scrape => &["stealth", "jina", "firecrawl", "browserless"],
        Mode::Similar => &["exa"],
        Mode::Social => &["xai"],
    }
}

pub async fn execute_search(
    ctx: Arc<AppContext>,
    query: &str,
    mode: Mode,
    count: usize,
    only_providers: &Option<Vec<String>>,
    opts: &SearchOpts,
) -> Result<SearchResponse, SearchError> {
    let start = Instant::now();
    let query_arc: Arc<str> = Arc::from(query);
    let timeout_budget = Duration::from_secs(ctx.config.settings.timeout.max(1));

    // Speculative Execution: If in Auto mode, we don't wait for classification 
    // to start the most likely providers (Brave, Serper).
    let mut speculative_set = JoinSet::new();
    let is_auto = mode == Mode::Auto;
    
    if is_auto && only_providers.is_none() {
        // Only speculate if we have keys and it's not a filtered provider list
        if !ctx.config.keys.brave.is_empty() {
            let q = query_arc.clone();
            let c = clamp_provider_count("brave", count);
            let o = opts.clone();
            let p = providers::brave::Brave::new(ctx.clone());
            speculative_set.spawn(async move {
                ("brave", timeout(timeout_budget, p.search(&q, c, &o)).await)
            });
        }
        if !ctx.config.keys.serper.is_empty() {
            let q = query_arc.clone();
            let c = count;
            let o = opts.clone();
            let p = providers::serper::Serper::new(ctx.clone());
            speculative_set.spawn(async move {
                ("serper", timeout(timeout_budget, p.search(&q, c, &o)).await)
            });
        }
    }

    let resolved_mode = if is_auto {
        classify_intent(query)
    } else {
        mode
    };

    // If auto resolved to a mode where Brave/Serper aren't wanted,
    // abort speculative tasks to avoid mixing generic web results into
    // intent-specific searches (e.g. news, social, academic).
    let spec_compatible = matches!(
        resolved_mode,
        Mode::Auto | Mode::General | Mode::Deep
    );
    if !spec_compatible {
        speculative_set.abort_all();
        // Drain aborted tasks so they don't merge later
        while speculative_set.join_next().await.is_some() {}
    }

    let all_providers = providers::build_providers(&ctx);
    let wanted = providers_for_mode(resolved_mode);

    let active: Vec<Box<dyn Provider>> = all_providers
        .into_iter()
        .filter(|p| {
            let name = p.name();
            // Don't restart speculative ones (they already launched above)
            if is_auto && only_providers.is_none() && (name == "brave" || name == "serper") { return false; }
            
            let in_mode_set = wanted.contains(&name);
            let in_filter = only_providers
                .as_ref()
                .map(|list| list.iter().any(|f| f.eq_ignore_ascii_case(name)))
                .unwrap_or(true);
            (in_mode_set || only_providers.is_some()) && in_filter && p.is_configured()
        })
        .collect();

    if active.is_empty() && speculative_set.is_empty() {
        return Err(SearchError::NoProviders(resolved_mode.to_string()));
    }

    let mut set = JoinSet::new();
    let mut providers_queried = Vec::new();

    // Re-add speculative ones to the tracking list (only if they weren't aborted)
    if is_auto && only_providers.is_none() && spec_compatible {
        if !ctx.config.keys.brave.is_empty() { providers_queried.push("brave".to_string()); }
        if !ctx.config.keys.serper.is_empty() { providers_queried.push("serper".to_string()); }
    }

    // For Deep mode, also launch Brave LLM Context API in parallel
    if resolved_mode == Mode::Deep && !ctx.config.keys.brave.is_empty() {
        let q = query_arc.clone();
        let c = count;
        let o = opts.clone();
        let brave = providers::brave::Brave::new(ctx.clone());
        set.spawn(async move {
            let result = timeout(timeout_budget, brave.search_llm_context(&q, c, &o)).await;
            ("brave_llm_context", result)
        });
        providers_queried.push("brave_llm_context".to_string());
    }

    for provider in active {
        let q = query_arc.clone();
        let name = provider.name();
        let c = clamp_provider_count(name, count);
        let sopts = opts.clone();
        providers_queried.push(name.to_string());

        match resolved_mode {
            Mode::News => {
                set.spawn(async move {
                    let result = timeout(timeout_budget, provider.search_news(&q, c, &sopts)).await;
                    (name, result)
                });
            }
            _ => {
                set.spawn(async move {
                    let result = timeout(timeout_budget, provider.search(&q, c, &sopts)).await;
                    (name, result)
                });
            }
        }
    }

    let mut all_results = Vec::new();
    let mut providers_failed = Vec::new();
    let mut providers_failed_detail: Vec<ProviderFailureDetail> = Vec::new();
    let mut unique_urls = HashSet::new();

    // Process speculative results first (they had a head start)
    while let Some(res) = speculative_set.join_next().await {
        match res {
            Ok((_name, Ok(Ok(items)))) => {
                for item in items {
                    if unique_urls.insert(normalize_url(&item.url)) {
                        all_results.push(item);
                    }
                }
            }
Ok((name, Ok(Err(e)))) => {
        tracing::warn!(event = "provider_failed", provider = name, mode = %resolved_mode, reason_code = e.error_code());
        tracing::warn!("{name} speculative failed: {e}");
        providers_failed.push(name.to_string());
        providers_failed_detail.push(failure_detail_from_error(name, &e));
    }
    Ok((name, Err(_))) => {
        tracing::warn!(event = "provider_timeout", provider = name, mode = %resolved_mode, reason_code = "timeout");
        tracing::warn!("{name} speculative timed out");
                providers_failed.push(name.to_string());
                providers_failed_detail.push(failure_detail_timeout(name));
            }
            Err(e) => {
                if !e.is_cancelled() {
                    tracing::error!("speculative join error: {e}");
                }
            }
        }
    }

    // Process the rest
    while let Some(join_result) = set.join_next().await {
        match join_result {
            Ok((_name, Ok(Ok(items)))) => {
                for item in items {
                    let normalized = normalize_url(&item.url);
                    if unique_urls.insert(normalized) {
                        all_results.push(item);
                    }
                }
                // If we already have enough results, cancel slow providers
                if all_results.len() >= count {
                    set.abort_all();
                    break;
                }
            }
Ok((name, Ok(Err(e)))) => {
        tracing::warn!(event = "provider_failed", provider = name, mode = %resolved_mode, reason_code = e.error_code());
        tracing::warn!("{name}: {e}");
        providers_failed.push(name.to_string());
        providers_failed_detail.push(failure_detail_from_error(name, &e));
    }
    Ok((name, Err(_))) => {
        tracing::warn!(event = "provider_timeout", provider = name, mode = %resolved_mode, reason_code = "timeout");
        tracing::warn!("{name}: timed out");
                providers_failed.push(name.to_string());
                providers_failed_detail.push(failure_detail_timeout(name));
            }
            Err(e) => {
                // JoinError from abort — not a real failure
                if !e.is_cancelled() {
                    tracing::error!("join error: {e}");
                }
            }
        }
    }

    // Trim to exact requested count
    all_results.truncate(count);

    let elapsed = start.elapsed();

    // Determine accurate status for agents
    let status = if all_results.is_empty() && !providers_failed.is_empty() {
        "all_providers_failed"
    } else if !all_results.is_empty() && !providers_failed.is_empty() {
        "partial_success"
    } else if all_results.is_empty() {
        "no_results"
    } else {
        "success"
    };

    Ok(finalize_response(SearchResponse {
        version: "1".into(),
        status: status.into(),
        query: query.to_string(),
        mode: resolved_mode.to_string(),
        results: all_results,
        metadata: ResponseMetadata {
            elapsed_ms: elapsed.as_millis(),
            result_count: 0,
            providers_queried,
            providers_failed,
            providers_failed_detail,
        },
    }))
}

/// Set result_count to match the actual results length.
fn finalize_response(mut response: SearchResponse) -> SearchResponse {
    response.metadata.result_count = response.results.len();
    response
}

/// Try a single provider call with a timeout budget, recording results/failures.
/// Returns true if the call produced results (caller may use this to short-circuit).
async fn try_provider<Fut>(
    name: &str,
    fut: Fut,
    timeout_budget: Duration,
    results: &mut Vec<SearchResult>,
    providers_queried: &mut Vec<String>,
    providers_failed: &mut Vec<String>,
    providers_failed_detail: &mut Vec<ProviderFailureDetail>,
) where
    Fut: std::future::Future<Output = Result<Vec<SearchResult>, SearchError>>,
{
    providers_queried.push(name.to_string());
    match tokio::time::timeout(timeout_budget, fut).await {
        Ok(Ok(items)) => results.extend(items),
        Ok(Err(e)) => {
            providers_failed.push(name.to_string());
            providers_failed_detail.push(failure_detail_from_error(name, &e));
            tracing::warn!("{name}: {e}");
        }
        Err(_) => {
            providers_failed.push(name.to_string());
            providers_failed_detail.push(failure_detail_timeout(name));
        }
    }
}

/// Like `try_provider` but uses a remaining deadline instead of a fixed budget.
/// Returns true if results were produced.
async fn try_provider_remaining<Fut>(
    name: &str,
    fut: Fut,
    remaining: Duration,
    results: &mut Vec<SearchResult>,
    providers_queried: &mut Vec<String>,
    providers_failed: &mut Vec<String>,
    providers_failed_detail: &mut Vec<ProviderFailureDetail>,
) where
    Fut: std::future::Future<Output = Result<Vec<SearchResult>, SearchError>>,
{
    if remaining.is_zero() {
        providers_queried.push(name.to_string());
        providers_failed.push(name.to_string());
        providers_failed_detail.push(failure_detail_timeout(name));
        return;
    }
    providers_queried.push(name.to_string());
    match tokio::time::timeout(remaining, fut).await {
        Ok(Ok(items)) => results.extend(items),
        Ok(Err(e)) => {
            providers_failed.push(name.to_string());
            providers_failed_detail.push(failure_detail_from_error(name, &e));
            tracing::warn!("{name}: {e}");
        }
        Err(_) => {
            providers_failed.push(name.to_string());
            providers_failed_detail.push(failure_detail_timeout(name));
        }
    }
}

fn normalize_url(url: &str) -> String {
    url.trim_end_matches('/')
        .replace("http://", "https://")
        .replace("www.", "")
        .to_lowercase()
}

fn provider_allowed(name: &str, only: &Option<Vec<String>>) -> bool {
    only.as_ref()
        .map(|list| list.iter().any(|f| f.eq_ignore_ascii_case(name)))
        .unwrap_or(true)
}

fn provider_count_cap(provider: &str) -> Option<usize> {
    // Brave Search API rejects high counts; clamp before dispatch.
    if provider.eq_ignore_ascii_case("brave") {
        Some(20)
    } else {
        None
    }
}

fn clamp_provider_count(provider: &str, requested: usize) -> usize {
    provider_count_cap(provider)
        .map(|cap| requested.min(cap))
        .unwrap_or(requested)
}

fn classify_failure_reason(err: &SearchError) -> &'static str {
    match err {
        SearchError::AuthMissing { .. } => "auth",
        SearchError::Config(_) | SearchError::NoProviders(_) => "validation",
        SearchError::Api { .. }
        | SearchError::RateLimited { .. }
        | SearchError::Http(_)
        | SearchError::Wreq(_) => "api",
        SearchError::Json(_) | SearchError::Io(_) => "unknown",
    }
}

fn failure_detail_from_error(provider: &str, err: &SearchError) -> ProviderFailureDetail {
    let classification = SearchError::classify_provider_error(provider, err.error_code());
    ProviderFailureDetail {
        provider: provider.to_string(),
        reason: classify_failure_reason(err).to_string(),
        code: err.error_code().to_string(),
        cause: classification.map(|c| c.cause.to_string()),
        action: classification.map(|c| c.action.to_string()),
        signature: classification.map(|c| c.signature.to_string()),
        message: Some(err.to_string()),
    }
}

fn failure_detail_timeout(provider: &str) -> ProviderFailureDetail {
    let classification = SearchError::classify_provider_error(provider, "timeout");
    ProviderFailureDetail {
        provider: provider.to_string(),
        reason: "timeout".to_string(),
        code: "timeout".to_string(),
        cause: classification.map(|c| c.cause.to_string()),
        action: classification.map(|c| c.action.to_string()),
        signature: classification.map(|c| c.signature.to_string()),
        message: None,
    }
}

/// Handle special modes that need direct provider method calls
pub async fn execute_special(
    ctx: Arc<AppContext>,
    query: &str,
    mode: Mode,
    count: usize,
    only_providers: &Option<Vec<String>>,
    _opts: &SearchOpts,
) -> Result<SearchResponse, SearchError> {
    let start = Instant::now();
    let timeout_budget = Duration::from_secs(ctx.config.settings.timeout.max(1));
    let mut results = Vec::new();
    let mut providers_queried = Vec::new();
    let mut providers_failed = Vec::new();
    let mut providers_failed_detail = Vec::new();

    match mode {
        Mode::Scholar => {
            let serper = providers::serper::Serper::new(ctx.clone());
            if serper.is_configured() && provider_allowed("serper", only_providers) {
                let pc = clamp_provider_count("serper", count);
                try_provider("serper", serper.search_scholar(query, pc), timeout_budget, &mut results, &mut providers_queried, &mut providers_failed, &mut providers_failed_detail).await;
            }
            let serpapi = providers::serpapi::SerpApi::new(ctx.clone());
            if serpapi.is_configured() && provider_allowed("serpapi", only_providers) {
                let pc = clamp_provider_count("serpapi", count);
                try_provider("serpapi", serpapi.search_scholar(query, pc), timeout_budget, &mut results, &mut providers_queried, &mut providers_failed, &mut providers_failed_detail).await;
            }
        }
        Mode::Patents => {
            let serper = providers::serper::Serper::new(ctx.clone());
            if serper.is_configured() && provider_allowed("serper", only_providers) {
                let pc = clamp_provider_count("serper", count);
                try_provider("serper", serper.search_patents(query, pc), timeout_budget, &mut results, &mut providers_queried, &mut providers_failed, &mut providers_failed_detail).await;
            }
        }
        Mode::Images => {
            let serper = providers::serper::Serper::new(ctx.clone());
            if serper.is_configured() && provider_allowed("serper", only_providers) {
                let pc = clamp_provider_count("serper", count);
                try_provider("serper", serper.search_images(query, pc), timeout_budget, &mut results, &mut providers_queried, &mut providers_failed, &mut providers_failed_detail).await;
            }
        }
        Mode::Places => {
            let serper = providers::serper::Serper::new(ctx.clone());
            if serper.is_configured() && provider_allowed("serper", only_providers) {
                let pc = clamp_provider_count("serper", count);
                try_provider("serper", serper.search_places(query, pc), timeout_budget, &mut results, &mut providers_queried, &mut providers_failed, &mut providers_failed_detail).await;
            }
        }
        Mode::People => {
            let exa = providers::exa::Exa::new(ctx.clone());
            if exa.is_configured() && provider_allowed("exa", only_providers) {
                let pc = clamp_provider_count("exa", count);
                try_provider("exa", exa.search_people(query, pc), timeout_budget, &mut results, &mut providers_queried, &mut providers_failed, &mut providers_failed_detail).await;
            }
        }
        Mode::Similar => {
            let exa = providers::exa::Exa::new(ctx.clone());
            if exa.is_configured() && provider_allowed("exa", only_providers) {
                let pc = clamp_provider_count("exa", count);
                try_provider("exa", exa.find_similar(query, pc), timeout_budget, &mut results, &mut providers_queried, &mut providers_failed, &mut providers_failed_detail).await;
            }
        }
        Mode::Social => {
            let xai = providers::xai::Xai::new(ctx.clone());
            if xai.is_configured() && provider_allowed("xai", only_providers) {
                let pc = clamp_provider_count("xai", count);
                try_provider("xai", xai.search(query, pc, _opts), timeout_budget, &mut results, &mut providers_queried, &mut providers_failed, &mut providers_failed_detail).await;
            }
        }
        Mode::Scrape | Mode::Extract => {
            // Shared deadline across the sequential fallback chain
            let deadline = Instant::now() + timeout_budget;

            let stealth = providers::stealth::Stealth::new(ctx.clone());
            if provider_allowed("stealth", only_providers) {
                let remaining = deadline.saturating_duration_since(Instant::now());
                try_provider_remaining("stealth", stealth.scrape_url(query), remaining, &mut results, &mut providers_queried, &mut providers_failed, &mut providers_failed_detail).await;
            }

            if results.is_empty() {
                let jina = providers::jina::Jina::new(ctx.clone());
                if jina.is_configured() && provider_allowed("jina", only_providers) {
                    let remaining = deadline.saturating_duration_since(Instant::now());
                    try_provider_remaining("jina", jina.read_url(query), remaining, &mut results, &mut providers_queried, &mut providers_failed, &mut providers_failed_detail).await;
                }
            }
            if results.is_empty() {
                let fc = providers::firecrawl::Firecrawl::new(ctx.clone());
                if fc.is_configured() && provider_allowed("firecrawl", only_providers) {
                    let remaining = deadline.saturating_duration_since(Instant::now());
                    try_provider_remaining("firecrawl", fc.scrape_url(query), remaining, &mut results, &mut providers_queried, &mut providers_failed, &mut providers_failed_detail).await;
                }
            }
            if results.is_empty() {
                let bl = providers::browserless::Browserless::new(ctx.clone());
                if bl.is_configured() && provider_allowed("browserless", only_providers) {
                    let remaining = deadline.saturating_duration_since(Instant::now());
                    try_provider_remaining("browserless", bl.scrape_url(query), remaining, &mut results, &mut providers_queried, &mut providers_failed, &mut providers_failed_detail).await;
                }
            }
        }
        _ => {} // handled by execute_search
    }

    if results.is_empty() && providers_queried.is_empty() {
        return Err(SearchError::NoProviders(mode.to_string()));
    }

    let elapsed = start.elapsed();

    let status = if results.is_empty() && !providers_failed.is_empty() {
        "all_providers_failed"
    } else if !results.is_empty() && !providers_failed.is_empty() {
        "partial_success"
    } else if results.is_empty() {
        "no_results"
    } else {
        "success"
    };

    Ok(finalize_response(SearchResponse {
        version: "1".into(),
        status: status.into(),
        query: query.to_string(),
        mode: mode.to_string(),
        results,
        metadata: ResponseMetadata {
            elapsed_ms: elapsed.as_millis(),
            result_count: 0,
            providers_queried,
            providers_failed,
            providers_failed_detail,
        },
    }))
}


/// Main dispatch: routes to execute_search or execute_special based on mode
pub async fn run(
    ctx: Arc<AppContext>,
    query: &str,
    mode: Mode,
    count: usize,
    only_providers: &Option<Vec<String>>,
    opts: &SearchOpts,
) -> Result<SearchResponse, SearchError> {
    // For Auto mode, check if it would resolve to a special mode.
    // If so, route to execute_special with the resolved mode.
    // Otherwise, pass Mode::Auto to execute_search so speculative execution works.
    let response = if mode == Mode::Auto {
        let resolved = classify_intent(query);
        match resolved {
            Mode::Scholar | Mode::Patents | Mode::Images | Mode::Places | Mode::People
            | Mode::Similar | Mode::Scrape | Mode::Extract | Mode::Social => {
                execute_special(ctx, query, resolved, count, only_providers, opts).await?
            }
            // Pass Auto to execute_search — it handles speculation + classification internally
            _ => execute_search(ctx, query, Mode::Auto, count, only_providers, opts).await?,
        }
    } else {
        match mode {
            Mode::Scholar | Mode::Patents | Mode::Images | Mode::Places | Mode::People
            | Mode::Similar | Mode::Scrape | Mode::Extract | Mode::Social => {
                execute_special(ctx, query, mode, count, only_providers, opts).await?
            }
            _ => execute_search(ctx, query, mode, count, only_providers, opts).await?,
        }
    };

    Ok(response)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clamp_provider_count_caps_brave_requests() {
        assert_eq!(clamp_provider_count("brave", 100), 20);
        assert_eq!(clamp_provider_count("brave", 20), 20);
        assert_eq!(clamp_provider_count("brave", 7), 7);
    }

    #[test]
    fn test_clamp_provider_count_preserves_uncapped_providers() {
        assert_eq!(clamp_provider_count("serper", 100), 100);
        assert_eq!(clamp_provider_count("exa", 42), 42);
    }

    #[test]
    fn test_normalize_url() {
        assert_eq!(normalize_url("http://www.example.com/"), "https://example.com");
        assert_eq!(normalize_url("https://example.com/path/"), "https://example.com/path");
        assert_eq!(normalize_url("https://example.com"), "https://example.com");
        assert_eq!(normalize_url("http://www.test.org/page"), "https://test.org/page");
        // lowercase is applied last, so WWW is lowered after www. strip
        assert_eq!(normalize_url("http://WWW.Example.COM/"), "https://www.example.com");
        // trailing slash on root
        assert_eq!(normalize_url("https://example.com/"), "https://example.com");
        // query parameters preserved
        assert_eq!(normalize_url("https://example.com/search?q=rust"), "https://example.com/search?q=rust");
        // fragment preserved
        assert_eq!(normalize_url("https://example.com/page#section"), "https://example.com/page#section");
        // already clean URL unchanged
        assert_eq!(normalize_url("https://example.com/clean"), "https://example.com/clean");
    }

    #[test]
    fn test_provider_allowed_no_filter() {
        assert!(provider_allowed("brave", &None));
        assert!(provider_allowed("any", &None));
    }

    #[test]
    fn test_provider_allowed_with_filter() {
        let only = Some(vec!["Brave".into(), "Exa".into()]);
        assert!(provider_allowed("brave", &only));
        assert!(provider_allowed("BRAVE", &only));
        assert!(provider_allowed("exa", &only));
        assert!(!provider_allowed("serper", &only));
    }
}
