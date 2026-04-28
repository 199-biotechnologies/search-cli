use crate::types::{ErrorDetail, ErrorResponse};
use thiserror::Error;

#[derive(Debug, Clone, Copy)]
pub struct RejectionClassification {
    pub cause: &'static str,
    pub action: &'static str,
    pub signature: &'static str,
}

#[derive(Error, Debug)]
pub enum SearchError {
    #[error("API error from {provider}: {message}")]
    Api {
        provider: &'static str,
        code: &'static str,
        message: String,
    },

    #[error("Authentication missing for {provider}")]
    AuthMissing { provider: &'static str },

    #[error("Rate limited by {provider}")]
    RateLimited { provider: &'static str },

    #[error("Configuration error: {0}")]
    Config(String),

    #[error("No providers configured for mode '{0}'")]
    NoProviders(String),

    #[error(transparent)]
    Http(#[from] reqwest::Error),

    #[error(transparent)]
    Wreq(#[from] wreq::Error),

    #[error(transparent)]
    Json(#[from] serde_json::Error),

    #[error(transparent)]
    Io(#[from] std::io::Error),
}

impl SearchError {
    pub fn classify_rejection(&self) -> Option<RejectionClassification> {
        match self {
            Self::Api {
                provider: "exa",
                code: "num_results_exceeded",
                ..
            } => Some(RejectionClassification {
                cause: "provider_limit_exceeded",
                action: "Lower -c/--count to provider supported range (Exa max results).",
                signature: "exa.NUM_RESULTS_EXCEEDED",
            }),
            Self::Api {
                provider: "jina",
                code: "cloudflare_1010",
                ..
            } => Some(RejectionClassification {
                cause: "provider_access_denied",
                action: "Switch provider or use extract/scrape fallback providers for this target.",
                signature: "jina.cloudflare_1010",
            }),
            Self::Api {
                provider: "browserless",
                code: "auth_mode_mismatch",
                ..
            } => Some(RejectionClassification {
                cause: "provider_auth_mode_mismatch",
                action: "Use the expected Browserless endpoint/auth mode and verify API key configuration.",
                signature: "browserless.auth_mode_mismatch",
            }),
            Self::RateLimited { .. } => Some(RejectionClassification {
                cause: "rate_limited",
                action: "Retry later or switch providers.",
                signature: "provider.rate_limited",
            }),
            Self::AuthMissing { .. } => Some(RejectionClassification {
                cause: "auth_missing",
                action: "Configure provider API key via env var or `search config set keys.<provider> ...`.",
                signature: "provider.auth_missing",
            }),
            Self::Api { .. } | Self::Http(_) | Self::Wreq(_) => Some(RejectionClassification {
                cause: "provider_api_error",
                action: "Retry with another provider or adjust query/mode parameters.",
                signature: "provider.api_error",
            }),
            _ => None,
        }
    }

    pub fn exit_code(&self) -> i32 {
        match self {
            Self::Config(_) | Self::NoProviders(_) => 2,
            Self::AuthMissing { .. } => 2,
            Self::RateLimited { .. } => 4,
            Self::Api { .. } | Self::Http(_) | Self::Wreq(_) => 1,
            Self::Json(_) | Self::Io(_) => 1,
        }
    }

    pub fn error_code(&self) -> &'static str {
        match self {
            Self::Api { code, .. } => code,
            Self::AuthMissing { .. } => "auth_missing",
            Self::RateLimited { .. } => "rate_limited",
            Self::Config(_) => "config_error",
            Self::NoProviders(_) => "no_providers",
            Self::Http(_) | Self::Wreq(_) => "http_error",
            Self::Json(_) => "json_error",
            Self::Io(_) => "io_error",
        }
    }

    pub fn suggestion(&self) -> Option<String> {
        match self {
            Self::AuthMissing { provider } => Some(format!(
                "Set {}_API_KEY env var or run: search config set keys.{} YOUR_KEY",
                provider.to_uppercase(),
                provider
            )),
            Self::NoProviders(mode) => {
                Some(format!(
                    "No providers configured for mode '{}'. Run: search config check",
                    mode
                ))
            }
            Self::RateLimited { provider } => Some(format!(
                "Rate limited by {}. Wait and retry, or use a different provider: search -p <other>",
                provider
            )),
            _ => None,
        }
    }

    pub fn to_error_response(&self) -> ErrorResponse {
        let classification = self.classify_rejection();
        ErrorResponse {
            version: "1",
            status: "error",
            error: ErrorDetail {
                code: self.error_code().to_string(),
                message: self.to_string(),
                cause: classification.map(|c| c.cause.to_string()),
                action: classification.map(|c| c.action.to_string()),
                signature: classification.map(|c| c.signature.to_string()),
                suggestion: self.suggestion(),
            },
        }
    }

    pub fn classify_provider_error(provider: &str, code: &str) -> Option<RejectionClassification> {
        match (provider, code) {
            ("exa", "num_results_exceeded") => Some(RejectionClassification {
                cause: "provider_limit_exceeded",
                action: "Lower -c/--count to provider supported range (Exa max results).",
                signature: "exa.NUM_RESULTS_EXCEEDED",
            }),
            ("jina", "cloudflare_1010") => Some(RejectionClassification {
                cause: "provider_access_denied",
                action: "Switch provider or use extract/scrape fallback providers for this target.",
                signature: "jina.cloudflare_1010",
            }),
            ("browserless", "auth_mode_mismatch") => Some(RejectionClassification {
                cause: "provider_auth_mode_mismatch",
                action: "Use the expected Browserless endpoint/auth mode and verify API key configuration.",
                signature: "browserless.auth_mode_mismatch",
            }),
            (_, "timeout") => Some(RejectionClassification {
                cause: "timeout",
                action: "Increase settings.timeout or switch to faster providers/modes.",
                signature: "provider.timeout",
            }),
            (_, "rate_limited") => Some(RejectionClassification {
                cause: "rate_limited",
                action: "Retry later or switch providers.",
                signature: "provider.rate_limited",
            }),
            (_, "auth_missing") => Some(RejectionClassification {
                cause: "auth_missing",
                action: "Configure provider API key via env var or `search config set keys.<provider> ...`.",
                signature: "provider.auth_missing",
            }),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_classify_exa_num_results_exceeded() {
        let err = SearchError::Api {
            provider: "exa",
            code: "num_results_exceeded",
            message: "NUM_RESULTS_EXCEEDED".to_string(),
        };
        let c = err.classify_rejection().expect("classification expected");
        assert_eq!(c.cause, "provider_limit_exceeded");
    }

    #[test]
    fn test_classify_jina_cloudflare_1010() {
        let err = SearchError::Api {
            provider: "jina",
            code: "cloudflare_1010",
            message: "Cloudflare 1010".to_string(),
        };
        let c = err.classify_rejection().expect("classification expected");
        assert_eq!(c.cause, "provider_access_denied");
    }

    #[test]
    fn test_classify_browserless_auth_mode_mismatch() {
        let err = SearchError::Api {
            provider: "browserless",
            code: "auth_mode_mismatch",
            message: "auth mode mismatch".to_string(),
        };
        let c = err.classify_rejection().expect("classification expected");
        assert_eq!(c.cause, "provider_auth_mode_mismatch");
    }

    #[test]
    fn test_classify_provider_error_timeout() {
        let c = SearchError::classify_provider_error("exa", "timeout").expect("classification expected");
        assert_eq!(c.cause, "timeout");
        assert_eq!(c.signature, "provider.timeout");
    }
}
