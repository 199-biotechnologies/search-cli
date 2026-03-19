use crate::types::SearchResponse;
use directories::ProjectDirs;
use std::path::PathBuf;

fn cache_dir() -> PathBuf {
    if let Some(proj) = ProjectDirs::from("", "", "search") {
        proj.cache_dir().to_path_buf()
    } else {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
        PathBuf::from(home).join(".cache").join("search")
    }
}

fn cache_path() -> PathBuf {
    cache_dir().join("last.json")
}

pub fn save_last(response: &SearchResponse) {
    if let Ok(json) = serde_json::to_string_pretty(response) {
        let dir = cache_dir();
        let _ = std::fs::create_dir_all(&dir);
        let _ = std::fs::write(cache_path(), json);
    }
}

pub fn load_last() -> Option<SearchResponse> {
    let path = cache_path();
    let content = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&content).ok()
}
