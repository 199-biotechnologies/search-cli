use crate::context::AppContext;
use crate::errors::SearchError;
use crate::providers::extract_title;
use crate::types::{SearchOpts, SearchResult};
use async_trait::async_trait;
use wreq::header::{HeaderMap, HeaderValue};
use wreq_util::Emulation;
use std::sync::Arc;
use std::time::Duration;
use url::Url;

pub struct Stealth {
    _ctx: Arc<AppContext>,
}

impl Stealth {
    pub fn new(ctx: Arc<AppContext>) -> Self {
        Self { _ctx: ctx }
    }

    /// Build a wreq client that impersonates Chrome with full TLS fingerprint.
    /// Timeout is sourced from unified config timeout budget.
    fn build_client(timeout_secs: u64) -> Result<wreq::Client, SearchError> {
        let mut headers = HeaderMap::new();

        // Chrome 136 stealth headers (matches Scrapling's browserforge output)
        headers.insert(
            "Accept",
            HeaderValue::from_static(
                "text/html,application/xhtml+xml,application/xml;q=0.9,image/avif,image/webp,image/apng,*/*;q=0.8",
            ),
        );
        headers.insert(
            "Accept-Language",
            HeaderValue::from_static("en-US,en;q=0.9"),
        );
        headers.insert(
            "Sec-Ch-Ua",
            HeaderValue::from_static(
                r#""Chromium";v="136", "Not_A Brand";v="24", "Google Chrome";v="136""#,
            ),
        );
        headers.insert("Sec-Ch-Ua-Mobile", HeaderValue::from_static("?0"));
        headers.insert(
            "Sec-Ch-Ua-Platform",
            HeaderValue::from_static(r#""macOS""#),
        );
        headers.insert("Sec-Fetch-Dest", HeaderValue::from_static("document"));
        headers.insert("Sec-Fetch-Mode", HeaderValue::from_static("navigate"));
        headers.insert("Sec-Fetch-Site", HeaderValue::from_static("none"));
        headers.insert("Sec-Fetch-User", HeaderValue::from_static("?1"));
        headers.insert(
            "Upgrade-Insecure-Requests",
            HeaderValue::from_static("1"),
        );
        headers.insert("DNT", HeaderValue::from_static("1"));
        headers.insert(
            "Cache-Control",
            HeaderValue::from_static("max-age=0"),
        );
        headers.insert(
            "Accept-Encoding",
            HeaderValue::from_static("gzip, deflate, br"),
        );

        wreq::Client::builder()
            .emulation(Emulation::Chrome136)
            .default_headers(headers)
            .timeout(Duration::from_secs(timeout_secs))
            .build()
            .map_err(|e| SearchError::Config(format!("Failed to build stealth client: {e}")))
    }

    /// Generate a convincing Google referer (Scrapling technique)
    fn google_referer(url: &Url) -> Option<String> {
        url.domain().map(|d| {
            let domain = d.trim_start_matches("www.");
            format!("https://www.google.com/search?q={domain}")
        })
    }

    pub async fn scrape_url(&self, url_str: &str) -> Result<Vec<SearchResult>, SearchError> {
        let client = Self::build_client(self._ctx.config.settings.timeout)?;
        let url = match Url::parse(url_str) {
            Ok(u) => u,
            Err(e) => return Err(SearchError::Config(format!("Invalid URL: {e}"))),
        };

        // Validate URL to prevent SSRF against internal services
        let host = url.host_str().unwrap_or_default();

        // Only allow https URLs
        if url.scheme() != "https" {
            return Err(SearchError::Api {
                provider: "stealth",
                code: "invalid_url",
                message: "Only HTTPS URLs are supported for scraping".into(),
            });
        }

        // Block private/internal IP ranges
        if let Ok(ip) = host.parse::<std::net::IpAddr>() {
            let is_internal = match ip {
                std::net::IpAddr::V4(v4) => v4.is_loopback() || v4.is_private() || v4.is_multicast(),
                std::net::IpAddr::V6(v6) => v6.is_loopback() || v6.is_multicast(),
            };
            if is_internal {
                return Err(SearchError::Api {
                    provider: "stealth",
                    code: "forbidden",
                    message: format!("SSRF: blocking internal IP address: {host}"),
                });
            }
        }

        // Block suspicious hostnames that look like private IPs
        if host.starts_with("127.") || host.starts_with("192.168.")
            || host.starts_with("10.") || host.ends_with(".internal")
        {
            return Err(SearchError::Api {
                provider: "stealth",
                code: "forbidden",
                message: format!("SSRF: blocking internal hostname: {host}"),
            });
        }

        // Set referer to look like we came from Google (Scrapling technique)
        let mut req = client.get(url.clone());
        if let Some(referer) = Self::google_referer(&url) {
            req = req.header("Referer", referer);
        }

    let resp = req.send().await.map_err(|e| {
        SearchError::Api { provider: "stealth", code: "http_error", message: format!("Stealth request failed: {e}") }
    })?;

        if !resp.status().is_success() {
            return Err(SearchError::Api {
                provider: "stealth",
                code: "http_error",
                message: format!("HTTP {}", resp.status()),
            });
        }

        let final_url = url_str.to_string(); // use original URL (wreq may not expose final URL)
    let html_bytes = resp.bytes().await.map_err(|e| {
        SearchError::Api { provider: "stealth", code: "read_error", message: format!("Failed to read body: {e}") }
    })?;
        let html = String::from_utf8_lossy(&html_bytes).into_owned();

    // Offload extraction to blocking pool so heavy HTML parsing doesn't block
    // the async runtime worker. Uses tl-based extraction (no readability/reqwest).
    let html_for_extract = html;
    let fallback_title = url_str.to_string();
    let (title, text) = tokio::task::spawn_blocking(move || {
        let title = extract_title(&html_for_extract).unwrap_or_else(|| fallback_title.clone());
        let body = extract_text_fallback(&html_for_extract);
        (title, body)
    })
    .await
    .map_err(|e| SearchError::Api { provider: "stealth", code: "extraction_error", message: format!("Stealth extraction task failed: {e}") })?;

        if text.trim().is_empty() {
            return Err(SearchError::Api {
                provider: "stealth",
                code: "extraction_error",
                message: "Page returned no extractable text content".to_string(),
            });
        }

        Ok(vec![SearchResult {
            title,
            url: final_url,
            snippet: text,
            source: "stealth".to_string(),
            published: None,
            image_url: None,
            extra: None,
        }])
    }
}

/// Simple fallback: strip all HTML tags and return text
fn extract_text_fallback(html: &str) -> String {
    let mut text = String::with_capacity(html.len() / 3);
    let mut in_tag = false;
    let mut in_skip = false;
    let bytes = html.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'<' => {
                in_tag = true;
                // Check for <script or <style start
                let rest = &html[i..];
                if rest.len() > 7
                    && (rest[..7].eq_ignore_ascii_case("<script")
                        || rest[..6].eq_ignore_ascii_case("<style"))
                {
                    in_skip = true;
                }
                // Check for </script> or </style> end
                if in_skip
                    && rest.len() > 8
                    && (rest[..9].eq_ignore_ascii_case("</script>")
                        || rest[..8].eq_ignore_ascii_case("</style>"))
                {
                    in_skip = false;
                }
            }
            b'>' => {
                in_tag = false;
            }
            _ if !in_tag && !in_skip => text.push(bytes[i] as char),
            _ => {}
        }
        i += 1;
    }
    // Collapse whitespace
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[async_trait]
impl super::Provider for Stealth {
    fn name(&self) -> &'static str {
        "stealth"
    }

    fn capabilities(&self) -> &[&'static str] {
        &["scrape", "extract"]
    }

    fn env_keys(&self) -> &[&'static str] {
        &[] // No API key needed
    }

    fn is_configured(&self) -> bool {
        true // No API key needed — local scraper
    }


    async fn search(
        &self,
        _query: &str,
        _count: usize,
        _opts: &SearchOpts,
    ) -> Result<Vec<SearchResult>, SearchError> {
        Ok(vec![])
    }

    async fn search_news(
        &self,
        _query: &str,
        _count: usize,
        _opts: &SearchOpts,
    ) -> Result<Vec<SearchResult>, SearchError> {
        Ok(vec![])
    }
}
