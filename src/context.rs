use crate::config::AppConfig;
use crate::errors::SearchError;
use std::time::Duration;

pub struct AppContext {
    pub client: reqwest::Client,
    pub config: AppConfig,
}

impl AppContext {
    pub fn new(config: AppConfig) -> Result<Self, SearchError> {
        let client = reqwest::Client::builder()
            .pool_idle_timeout(Duration::from_secs(60))
            .tcp_nodelay(true)
            .timeout(Duration::from_secs(config.settings.timeout))
            .user_agent(format!("search-cli/{}", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|e| SearchError::Config(format!("failed to build HTTP client: {}", e)))?;

        Ok(Self { client, config })
    }
}
