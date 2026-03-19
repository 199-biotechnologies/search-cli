use crate::config::AppConfig;
use std::time::Duration;

pub struct AppContext {
    pub client: reqwest::Client,
    pub config: AppConfig,
}

impl AppContext {
    pub fn new(config: AppConfig) -> Self {
        let client = reqwest::Client::builder()
            .pool_idle_timeout(Duration::from_secs(60))
            .tcp_nodelay(true)
            .timeout(Duration::from_secs(config.settings.timeout))
            .user_agent(format!("search-cli/{}", env!("CARGO_PKG_VERSION")))
            .build()
            .expect("failed to build HTTP client");

        Self { client, config }
    }
}
