use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    /// Auto-detect intent from query (default)
    Auto,
    /// General web search (Brave + Serper + Exa + Jina)
    General,
    /// Breaking news and current events (Brave News + Serper News)
    News,
    /// Research papers and studies (Exa + Serper)
    Academic,
    /// Find people, LinkedIn profiles (Exa)
    People,
    /// Comprehensive multi-engine coverage (Exa + Serper)
    Deep,
    /// Extract full text content from a URL (Jina Reader -> Firecrawl)
    Extract,
    /// Find pages similar to a URL (Exa findSimilar)
    Similar,
    /// Scrape page content (Jina Reader -> Firecrawl)
    Scrape,
    /// Google Scholar search (Serper)
    Scholar,
    /// Patent search (Serper)
    Patents,
    /// Image search (Serper)
    Images,
    /// Local businesses and places (Serper)
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResponse {
    pub version: String,
    pub status: String,
    pub query: String,
    pub mode: String,
    pub results: Vec<SearchResult>,
    pub metadata: ResponseMetadata,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponseMetadata {
    pub elapsed_ms: u128,
    pub result_count: usize,
    pub providers_queried: Vec<String>,
    pub providers_failed: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct SearchOpts {
    pub include_domains: Vec<String>,
    pub exclude_domains: Vec<String>,
    /// day, week, month, year
    pub freshness: Option<String>,
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
