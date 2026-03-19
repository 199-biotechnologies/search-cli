use crate::classify::classify_intent;
use crate::context::AppContext;
use crate::errors::SearchError;
use crate::providers::{self, Provider};
use crate::types::{Mode, ResponseMetadata, SearchResponse, SearchResult};
use std::collections::HashSet;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::task::JoinSet;
use tokio::time::timeout;

/// Which providers to query for each mode
fn providers_for_mode(mode: Mode) -> &'static [&'static str] {
    match mode {
        Mode::Auto | Mode::General => &["brave", "serper", "exa", "jina"],
        Mode::News => &["brave", "serper"],
        Mode::Academic | Mode::Deep => &["exa", "serper"],
        Mode::Scholar => &["serper"],
        Mode::Patents => &["serper"],
        Mode::People => &["exa"],
        Mode::Images => &["serper"],
        Mode::Places => &["serper"],
        Mode::Extract | Mode::Scrape => &["jina", "firecrawl"],
        Mode::Similar => &["exa"],
    }
}

pub async fn execute_search(
    ctx: Arc<AppContext>,
    query: &str,
    mode: Mode,
    count: usize,
) -> Result<SearchResponse, SearchError> {
    let start = Instant::now();

    let resolved_mode = if mode == Mode::Auto {
        classify_intent(query)
    } else {
        mode
    };

    let all_providers = providers::build_providers(&ctx);
    let wanted = providers_for_mode(resolved_mode);

    let active: Vec<Box<dyn Provider>> = all_providers
        .into_iter()
        .filter(|p| wanted.contains(&p.name()) && p.is_configured())
        .collect();

    if active.is_empty() {
        return Err(SearchError::NoProviders(resolved_mode.to_string()));
    }

    let mut set = JoinSet::new();
    let mut providers_queried = Vec::new();

    for provider in active {
        let q = query.to_string();
        let c = count;
        let name = provider.name();
        let tout = provider.timeout();
        providers_queried.push(name.to_string());

        // Choose the right method based on mode
        match resolved_mode {
            Mode::News => {
                set.spawn(async move {
                    let result = timeout(tout, provider.search_news(&q, c)).await;
                    (name, result)
                });
            }
            _ => {
                set.spawn(async move {
                    let result = timeout(tout, provider.search(&q, c)).await;
                    (name, result)
                });
            }
        }
    }

    let mut all_results = Vec::new();
    let mut providers_failed = Vec::new();

    while let Some(join_result) = set.join_next().await {
        match join_result {
            Ok((_name, Ok(Ok(items)))) => {
                all_results.extend(items);
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
                tracing::error!("join error: {e}");
            }
        }
    }

    // Dedup by URL
    dedup_results(&mut all_results);

    // Trim to requested count
    all_results.truncate(count);

    let elapsed = start.elapsed();

    Ok(SearchResponse {
        version: "1",
        status: "success",
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

fn dedup_results(results: &mut Vec<SearchResult>) {
    let mut seen = HashSet::new();
    results.retain(|r| {
        let normalized = normalize_url(&r.url);
        seen.insert(normalized)
    });
}

fn normalize_url(url: &str) -> String {
    url.trim_end_matches('/')
        .replace("http://", "https://")
        .replace("www.", "")
        .to_lowercase()
}

/// Handle special modes that need direct provider method calls
pub async fn execute_special(
    ctx: Arc<AppContext>,
    query: &str,
    mode: Mode,
    count: usize,
) -> Result<SearchResponse, SearchError> {
    let start = Instant::now();
    let all_providers = providers::build_providers(&ctx);
    let mut results = Vec::new();
    let mut providers_queried = Vec::new();
    let mut providers_failed = Vec::new();

    match mode {
        Mode::Scholar => {
            for p in &all_providers {
                if p.name() == "serper" && p.is_configured() {
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
        }
        Mode::Patents => {
            let serper = providers::serper::Serper::new(ctx.clone());
            if serper.is_configured() {
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
            if serper.is_configured() {
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
            if serper.is_configured() {
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
            if exa.is_configured() {
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
            if exa.is_configured() {
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
        Mode::Scrape | Mode::Extract => {
            // Try Jina reader first, then Firecrawl
            let jina = providers::jina::Jina::new(ctx.clone());
            if jina.is_configured() {
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
            if results.is_empty() {
                let fc = providers::firecrawl::Firecrawl::new(ctx.clone());
                if fc.is_configured() {
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
        }
        _ => {} // handled by execute_search
    }

    if results.is_empty() && providers_queried.is_empty() {
        return Err(SearchError::NoProviders(mode.to_string()));
    }

    let elapsed = start.elapsed();
    let result_count = results.len();

    Ok(SearchResponse {
        version: "1",
        status: "success",
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
) -> Result<SearchResponse, SearchError> {
    let resolved_mode = if mode == Mode::Auto {
        classify_intent(query)
    } else {
        mode
    };

    let mut response = match resolved_mode {
        Mode::Scholar | Mode::Patents | Mode::Images | Mode::Places | Mode::People
        | Mode::Similar | Mode::Scrape | Mode::Extract => {
            execute_special(ctx, query, resolved_mode, count).await?
        }
        _ => execute_search(ctx, query, resolved_mode, count).await?,
    };

    response.metadata.result_count = response.results.len();
    Ok(response)
}
