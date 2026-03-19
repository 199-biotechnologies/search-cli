use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    Auto,
    General,
    News,
    Academic,
    People,
    Deep,
    Extract,
    Similar,
    Scrape,
    Scholar,
    Patents,
    Images,
    Places,
}

impl fmt::Display for Mode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Mode::Auto => "auto",
            Mode::General => "general",
            Mode::News => "news",
            Mode::Academic => "academic",
            Mode::People => "people",
            Mode::Deep => "deep",
            Mode::Extract => "extract",
            Mode::Similar => "similar",
            Mode::Scrape => "scrape",
            Mode::Scholar => "scholar",
            Mode::Patents => "patents",
            Mode::Images => "images",
            Mode::Places => "places",
        };
        write!(f, "{s}")
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    pub title: String,
    pub url: String,
    pub snippet: String,
    pub source: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub published: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extra: Option<serde_json::Value>,
}

#[derive(Debug, Serialize)]
pub struct SearchResponse {
    pub version: &'static str,
    pub status: &'static str,
    pub query: String,
    pub mode: String,
    pub results: Vec<SearchResult>,
    pub metadata: ResponseMetadata,
}

#[derive(Debug, Serialize)]
pub struct ResponseMetadata {
    pub elapsed_ms: u128,
    pub result_count: usize,
    pub providers_queried: Vec<String>,
    pub providers_failed: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct ErrorResponse {
    pub version: &'static str,
    pub status: &'static str,
    pub error: ErrorDetail,
}

#[derive(Debug, Serialize)]
pub struct ErrorDetail {
    pub code: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suggestion: Option<String>,
}
