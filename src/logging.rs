use crate::types::SearchResponse;
use directories::ProjectDirs;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;

fn log_dir() -> PathBuf {
    if let Some(proj) = ProjectDirs::from("", "", "search") {
        proj.data_dir().join("logs")
    } else {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
        PathBuf::from(home).join(".local").join("share").join("search").join("logs")
    }
}

fn log_path() -> PathBuf {
    // One log file per day: searches_2026-03-22.jsonl
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let days = now / 86400;
    let date = epoch_days_to_date(days);
    log_dir().join(format!("searches_{date}.jsonl"))
}

/// Log a completed search to the daily JSONL log file.
pub fn log_search(response: &SearchResponse) {
    let path = log_path();
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    // Build a compact log entry
    let entry = serde_json::json!({
        "ts": now,
        "query": response.query,
        "mode": response.mode,
        "result_count": response.metadata.result_count,
        "elapsed_ms": response.metadata.elapsed_ms,
        "providers_queried": response.metadata.providers_queried,
        "providers_failed": response.metadata.providers_failed,
        "sources": response.results.iter().map(|r| &r.source).collect::<Vec<_>>(),
        "urls": response.results.iter().take(10).map(|r| &r.url).collect::<Vec<_>>(),
    });

    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(&path) {
        let _ = writeln!(file, "{}", serde_json::to_string(&entry).unwrap_or_default());
    }
}

fn epoch_days_to_date(total_days: u64) -> String {
    let z = total_days as i64 + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}")
}
