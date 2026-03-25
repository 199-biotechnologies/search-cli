use crate::types::{ErrorDetail, ErrorResponse};
use thiserror::Error;

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
    Rquest(#[from] rquest::Error),

    #[error(transparent)]
    Json(#[from] serde_json::Error),

    #[error(transparent)]
    Io(#[from] std::io::Error),
}

impl SearchError {
    pub fn exit_code(&self) -> i32 {
        match self {
            Self::Config(_) | Self::NoProviders(_) => 2,
            Self::AuthMissing { .. } => 3,
            Self::RateLimited { .. } => 4,
            Self::Api { .. } | Self::Http(_) | Self::Rquest(_) => 1,
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
            Self::Http(_) | Self::Rquest(_) => "http_error",
            Self::Json(_) => "json_error",
            Self::Io(_) => "io_error",
        }
    }

    pub fn suggestion(&self) -> Option<String> {
        match self {
            Self::AuthMissing { provider } => Some(format!(
                "Set SEARCH_KEYS_{} env var or run: search config set keys.{} YOUR_KEY",
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
        ErrorResponse {
            version: "1",
            status: "error",
            error: ErrorDetail {
                code: self.error_code().to_string(),
                message: self.to_string(),
                suggestion: self.suggestion(),
            },
        }
    }
}
