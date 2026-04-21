use crate::types::SearchResponse;
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

const CACHE_TTL_SECS: u64 = 300; // 5 minutes

fn cache_dir() -> PathBuf {
    if let Some(proj) = ProjectDirs::from("", "", "search") {
        proj.cache_dir().to_path_buf()
    } else {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
        PathBuf::from(home).join(".cache").join("search")
    }
}

fn last_path() -> PathBuf {
    cache_dir().join("last.json")
}

fn query_cache_path(query: &str, mode: &str) -> PathBuf {
    let mut h = DefaultHasher::new();
    query.to_lowercase().hash(&mut h);
    mode.hash(&mut h);
    cache_dir().join(format!("q_{:x}.json", h.finish()))
}

pub fn save_last(response: &SearchResponse) {
    let dir = cache_dir();
    let _ = std::fs::create_dir_all(&dir);
    if let Ok(json) = serde_json::to_string(response) {
        let _ = std::fs::write(last_path(), &json);
    }
}

pub fn load_last() -> Option<SearchResponse> {
    let content = std::fs::read_to_string(last_path()).ok()?;
    serde_json::from_str(&content).ok()
}

#[derive(Serialize, Deserialize)]
struct CachedEntry {
    timestamp: u64,
    response: SearchResponse,
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Returns whether a response is safe/useful to persist in query cache.
///
/// We intentionally skip caching failure artifacts so repeated queries do not
/// replay stale failed/degraded-empty responses.
fn should_cache_query_response(response: &SearchResponse) -> bool {
    // Explicit provider-failure terminal state.
    if response.status == "all_providers_failed" {
        return false;
    }

    // Defensive degraded-empty check (0 results with provider failures), even
    // if status naming changes in the future.
    if response.results.is_empty() && !response.metadata.providers_failed.is_empty() {
        return false;
    }

    true
}

/// Save a query result to the TTL cache
pub fn save_query(query: &str, mode: &str, response: &SearchResponse) {
    if !should_cache_query_response(response) {
        return;
    }

    let dir = cache_dir();
    let _ = std::fs::create_dir_all(&dir);
    let entry = CachedEntry {
        timestamp: now_secs(),
        response: response.clone(),
    };
    if let Ok(json) = serde_json::to_string(&entry) {
        let _ = std::fs::write(query_cache_path(query, mode), json);
    }
}

/// Load a cached query result if not expired
pub fn load_query(query: &str, mode: &str) -> Option<SearchResponse> {
    let path = query_cache_path(query, mode);
    let content = std::fs::read_to_string(path).ok()?;
    let entry: CachedEntry = serde_json::from_str(&content).ok()?;
    if now_secs() - entry.timestamp < CACHE_TTL_SECS {
        Some(entry.response)
    } else {
        None // expired
    }
}
