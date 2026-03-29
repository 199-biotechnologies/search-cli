use crate::classify::classify_intent;
use crate::context::AppContext;
use crate::errors::SearchError;
use crate::providers::{self, Provider};
use crate::types::{Mode, ResponseMetadata, SearchOpts, SearchResponse};
use std::collections::HashSet;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::task::JoinSet;
use tokio::time::timeout;

/// Which providers to query for each mode
fn providers_for_mode(mode: Mode) -> &'static [&'static str] {
    match mode {
        Mode::Auto | Mode::General => &["parallel", "brave", "serper", "exa", "jina", "tavily", "perplexity"],
        Mode::News => &["parallel", "brave", "serper", "tavily", "perplexity"],
        Mode::Academic => &["exa", "serper", "tavily", "perplexity"],
        Mode::Deep => &["parallel", "brave", "exa", "serper", "tavily", "perplexity", "xai"],
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

    // Speculative Execution: If in Auto mode, we don't wait for classification 
    // to start the most likely providers (Brave, Serper).
    let mut speculative_set = JoinSet::new();
    let is_auto = mode == Mode::Auto;
    
    if is_auto && only_providers.is_none() {
        // Only speculate if we have keys and it's not a filtered provider list
        if !ctx.config.keys.brave.is_empty() {
            let q = query_arc.clone();
            let c = count;
            let o = opts.clone();
            let p = providers::brave::Brave::new(ctx.clone());
            speculative_set.spawn(async move { ("brave", p.search(&q, c, &o).await) });
        }
        if !ctx.config.keys.serper.is_empty() {
            let q = query_arc.clone();
            let c = count;
            let o = opts.clone();
            let p = providers::serper::Serper::new(ctx.clone());
            speculative_set.spawn(async move { ("serper", p.search(&q, c, &o).await) });
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
            let result = timeout(Duration::from_secs(15), brave.search_llm_context(&q, c, &o)).await;
            ("brave_llm_context", result)
        });
        providers_queried.push("brave_llm_context".to_string());
    }

    for provider in active {
        let q = query_arc.clone();
        let c = count;
        let name = provider.name();
        let tout = provider.timeout();
        let sopts = opts.clone();
        providers_queried.push(name.to_string());

        match resolved_mode {
            Mode::News => {
                set.spawn(async move {
                    let result = timeout(tout, provider.search_news(&q, c, &sopts)).await;
                    (name, result)
                });
            }
            _ => {
                set.spawn(async move {
                    let result = timeout(tout, provider.search(&q, c, &sopts)).await;
                    (name, result)
                });
            }
        }
    }

    let mut all_results = Vec::new();
    let mut providers_failed = Vec::new();
    let mut unique_urls = HashSet::new();

    // Process speculative results first (they had a head start)
    while let Some(res) = speculative_set.join_next().await {
        if let Ok((_name, Ok(items))) = res {
            for item in items {
                if unique_urls.insert(normalize_url(&item.url)) {
                    all_results.push(item);
                }
            }
        } else if let Ok((name, Err(e))) = res {
            tracing::warn!("{name} speculative failed: {e}");
            providers_failed.push(name.to_string());
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
                tracing::warn!("{name}: {e}");
                providers_failed.push(name.to_string());
            }
            Ok((name, Err(_))) => {
                tracing::warn!("{name}: timed out");
                providers_failed.push(name.to_string());
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

    Ok(SearchResponse {
        version: "1".into(),
        status: status.into(),
        query: query.to_string(),
        mode: resolved_mode.to_string(),
        results: all_results,
        metadata: ResponseMetadata {
            elapsed_ms: elapsed.as_millis(),
            result_count: 0, // will be set below
            providers_queried,
            providers_failed,
        },
    })
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
    let all_providers = providers::build_providers(&ctx);
    let mut results = Vec::new();
    let mut providers_queried = Vec::new();
    let mut providers_failed = Vec::new();

    match mode {
        Mode::Scholar => {
            for p in &all_providers {
                if p.name() == "serper" && p.is_configured() && provider_allowed("serper", only_providers) {
                    providers_queried.push("serper".to_string());
                    // Downcast to Serper for scholar-specific method
                    let serper = providers::serper::Serper::new(ctx.clone());
                    match timeout(p.timeout(), serper.search_scholar(query, count)).await {
                        Ok(Ok(items)) => results.extend(items),
                        Ok(Err(e)) => {
                            providers_failed.push("serper".to_string());
                            tracing::warn!("serper scholar: {e}");
                        }
                        Err(_) => {
                            providers_failed.push("serper".to_string());
                        }
                    }
                }
            }
            // Also try SerpApi for scholar
            let serpapi = providers::serpapi::SerpApi::new(ctx.clone());
            if serpapi.is_configured() && provider_allowed("serpapi", only_providers) {
                providers_queried.push("serpapi".to_string());
                match timeout(Duration::from_secs(10), serpapi.search_scholar(query, count)).await {
                    Ok(Ok(items)) => results.extend(items),
                    Ok(Err(e)) => {
                        providers_failed.push("serpapi".to_string());
                        tracing::warn!("serpapi scholar: {e}");
                    }
                    Err(_) => {
                        providers_failed.push("serpapi".to_string());
                    }
                }
            }
        }
        Mode::Patents => {
            let serper = providers::serper::Serper::new(ctx.clone());
            if serper.is_configured() && provider_allowed("serper", only_providers) {
                providers_queried.push("serper".to_string());
                match timeout(Duration::from_secs(10), serper.search_patents(query, count)).await {
                    Ok(Ok(items)) => results.extend(items),
                    Ok(Err(e)) => {
                        providers_failed.push("serper".to_string());
                        tracing::warn!("serper patents: {e}");
                    }
                    Err(_) => providers_failed.push("serper".to_string()),
                }
            }
        }
        Mode::Images => {
            let serper = providers::serper::Serper::new(ctx.clone());
            if serper.is_configured() && provider_allowed("serper", only_providers) {
                providers_queried.push("serper".to_string());
                match timeout(Duration::from_secs(10), serper.search_images(query, count)).await {
                    Ok(Ok(items)) => results.extend(items),
                    Ok(Err(e)) => {
                        providers_failed.push("serper".to_string());
                        tracing::warn!("serper images: {e}");
                    }
                    Err(_) => providers_failed.push("serper".to_string()),
                }
            }
        }
        Mode::Places => {
            let serper = providers::serper::Serper::new(ctx.clone());
            if serper.is_configured() && provider_allowed("serper", only_providers) {
                providers_queried.push("serper".to_string());
                match timeout(Duration::from_secs(10), serper.search_places(query, count)).await {
                    Ok(Ok(items)) => results.extend(items),
                    Ok(Err(e)) => {
                        providers_failed.push("serper".to_string());
                        tracing::warn!("serper places: {e}");
                    }
                    Err(_) => providers_failed.push("serper".to_string()),
                }
            }
        }
        Mode::People => {
            let exa = providers::exa::Exa::new(ctx.clone());
            if exa.is_configured() && provider_allowed("exa", only_providers) {
                providers_queried.push("exa".to_string());
                match timeout(Duration::from_secs(15), exa.search_people(query, count)).await {
                    Ok(Ok(items)) => results.extend(items),
                    Ok(Err(e)) => {
                        providers_failed.push("exa".to_string());
                        tracing::warn!("exa people: {e}");
                    }
                    Err(_) => providers_failed.push("exa".to_string()),
                }
            }
        }
        Mode::Similar => {
            let exa = providers::exa::Exa::new(ctx.clone());
            if exa.is_configured() && provider_allowed("exa", only_providers) {
                providers_queried.push("exa".to_string());
                match timeout(Duration::from_secs(15), exa.find_similar(query, count)).await {
                    Ok(Ok(items)) => results.extend(items),
                    Ok(Err(e)) => {
                        providers_failed.push("exa".to_string());
                        tracing::warn!("exa similar: {e}");
                    }
                    Err(_) => providers_failed.push("exa".to_string()),
                }
            }
        }
        Mode::Social => {
            let xai = providers::xai::Xai::new(ctx.clone());
            if xai.is_configured() && provider_allowed("xai", only_providers) {
                providers_queried.push("xai".to_string());
                match timeout(Duration::from_secs(60), xai.search(query, count, _opts)).await {
                    Ok(Ok(items)) => results.extend(items),
                    Ok(Err(e)) => {
                        providers_failed.push("xai".to_string());
                        tracing::warn!("xai: {e}");
                    }
                    Err(_) => providers_failed.push("xai".to_string()),
                }
            }
        }
        Mode::Scrape | Mode::Extract => {
            // Try Stealth (local) first, then Jina reader, then Firecrawl
            let stealth = providers::stealth::Stealth::new(ctx.clone());
            if provider_allowed("stealth", only_providers) {
                providers_queried.push("stealth".to_string());
                match timeout(Duration::from_secs(30), stealth.scrape_url(query)).await {
                    Ok(Ok(items)) => results.extend(items),
                    Ok(Err(e)) => {
                        providers_failed.push("stealth".to_string());
                        tracing::warn!("stealth: {e}");
                    }
                    Err(_) => providers_failed.push("stealth".to_string()),
                }
            }

            if results.is_empty() {
                let jina = providers::jina::Jina::new(ctx.clone());
                if jina.is_configured() && provider_allowed("jina", only_providers) {
                    providers_queried.push("jina".to_string());
                    match timeout(Duration::from_secs(30), jina.read_url(query)).await {
                        Ok(Ok(items)) => results.extend(items),
                        Ok(Err(e)) => {
                            providers_failed.push("jina".to_string());
                            tracing::warn!("jina reader: {e}");
                        }
                        Err(_) => providers_failed.push("jina".to_string()),
                    }
                }
            }
            if results.is_empty() {
                let fc = providers::firecrawl::Firecrawl::new(ctx.clone());
                if fc.is_configured() && provider_allowed("firecrawl", only_providers) {
                    providers_queried.push("firecrawl".to_string());
                    match timeout(Duration::from_secs(30), fc.scrape_url(query)).await {
                        Ok(Ok(items)) => results.extend(items),
                        Ok(Err(e)) => {
                            providers_failed.push("firecrawl".to_string());
                            tracing::warn!("firecrawl: {e}");
                        }
                        Err(_) => providers_failed.push("firecrawl".to_string()),
                    }
                }
            }
            // Last resort: Browserless cloud browser (handles Cloudflare, JS rendering)
            if results.is_empty() {
                let bl = providers::browserless::Browserless::new(ctx.clone());
                if bl.is_configured() && provider_allowed("browserless", only_providers) {
                    providers_queried.push("browserless".to_string());
                    match timeout(Duration::from_secs(30), bl.scrape_url(query)).await {
                        Ok(Ok(items)) => results.extend(items),
                        Ok(Err(e)) => {
                            providers_failed.push("browserless".to_string());
                            tracing::warn!("browserless: {e}");
                        }
                        Err(_) => providers_failed.push("browserless".to_string()),
                    }
                }
            }
        }
        _ => {} // handled by execute_search
    }

    if results.is_empty() && providers_queried.is_empty() {
        return Err(SearchError::NoProviders(mode.to_string()));
    }

    let elapsed = start.elapsed();
    let result_count = results.len();

    let status = if results.is_empty() && !providers_failed.is_empty() {
        "all_providers_failed"
    } else if !results.is_empty() && !providers_failed.is_empty() {
        "partial_success"
    } else if results.is_empty() {
        "no_results"
    } else {
        "success"
    };

    Ok(SearchResponse {
        version: "1".into(),
        status: status.into(),
        query: query.to_string(),
        mode: mode.to_string(),
        results,
        metadata: ResponseMetadata {
            elapsed_ms: elapsed.as_millis(),
            result_count,
            providers_queried,
            providers_failed,
        },
    })
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
    let mut response = if mode == Mode::Auto {
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

    response.metadata.result_count = response.results.len();
    Ok(response)
}
