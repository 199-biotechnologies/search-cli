use directories::ProjectDirs;
use figment::{
    providers::{Env, Format, Serialized, Toml},
    Figment,
};
use serde::{Deserialize, Deserializer, Serialize};
use std::path::PathBuf;

/// Deserialize a u64 that tolerates legacy quoted numeric strings (e.g., timeout = "77").
/// Coercion is only applied to string values that parse as u64; other strings fail clearly.
fn deserialize_u64_tolerant<'de, D>(deserializer: D) -> Result<u64, D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum RawU64 {
        Native(u64),
        Quoted(String),
    }

    let raw = RawU64::deserialize(deserializer)?;
    match raw {
        RawU64::Native(v) => Ok(v),
        RawU64::Quoted(s) => s.parse::<u64>().map_err(|e| {
            serde::de::Error::custom(format!("invalid numeric value: '{}' - {}", s, e))
        }),
    }
}

/// Deserialize a usize that tolerates legacy quoted numeric strings (e.g., count = "15").
/// Coercion is only applied to string values that parse as usize; other strings fail clearly.
fn deserialize_usize_tolerant<'de, D>(deserializer: D) -> Result<usize, D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum RawUsize {
        Native(usize),
        Quoted(String),
    }

    let raw = RawUsize::deserialize(deserializer)?;
    match raw {
        RawUsize::Native(v) => Ok(v),
        RawUsize::Quoted(s) => s.parse::<usize>().map_err(|e| {
            serde::de::Error::custom(format!("invalid numeric value: '{}' - {}", s, e))
        }),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub keys: ApiKeys,
    pub settings: Settings,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiKeys {
    #[serde(default)]
    pub parallel: String,
    #[serde(default)]
    pub brave: String,
    #[serde(default)]
    pub serper: String,
    #[serde(default)]
    pub exa: String,
    #[serde(default)]
    pub jina: String,
    #[serde(default)]
    pub firecrawl: String,
    #[serde(default)]
    pub tavily: String,
    #[serde(default)]
    pub serpapi: String,
    #[serde(default)]
    pub perplexity: String,
    #[serde(default)]
    pub browserless: String,
    #[serde(default)]
    pub xai: String,
    #[serde(default)]
    pub you: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    #[serde(default = "default_timeout", deserialize_with = "deserialize_u64_tolerant")]
    pub timeout: u64,
    #[serde(default = "default_count", deserialize_with = "deserialize_usize_tolerant")]
    pub count: usize,
    #[serde(default = "default_retry_count", deserialize_with = "deserialize_usize_tolerant")]
    pub retry_count: usize,
    #[serde(default = "default_min_results", deserialize_with = "deserialize_usize_tolerant")]
    pub min_results: usize,
    #[serde(default = "default_provider_timeout", deserialize_with = "deserialize_u64_tolerant")]
    pub provider_timeout: u64,
}

fn default_timeout() -> u64 { 30 }
fn default_count() -> usize { 10 }
fn default_retry_count() -> usize { 3 }
fn default_min_results() -> usize { 0 }
fn default_provider_timeout() -> u64 { 0 }

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            keys: ApiKeys {
                parallel: String::new(),
                brave: String::new(),
                serper: String::new(),
                exa: String::new(),
                jina: String::new(),
                firecrawl: String::new(),
                tavily: String::new(),
                serpapi: String::new(),
                perplexity: String::new(),
                browserless: String::new(),
                xai: String::new(),
                you: String::new(),
            },
            settings: Settings {
                timeout: default_timeout(),
                count: default_count(),
                retry_count: default_retry_count(),
                min_results: default_min_results(),
                provider_timeout: default_provider_timeout(),
            },
        }
    }
}

pub fn config_dir() -> PathBuf {
    if let Some(proj) = ProjectDirs::from("", "", "search") {
        proj.config_dir().to_path_buf()
    } else {
        dirs_fallback()
    }
}

/// Cross-platform home directory: $HOME on Unix, %USERPROFILE% on Windows.
pub fn home_dir() -> PathBuf {
    std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."))
}

fn dirs_fallback() -> PathBuf {
    home_dir().join(".config").join("search")
}

pub fn config_path() -> PathBuf {
    config_dir().join("config.toml")
}

pub fn load_config() -> Result<AppConfig, Box<figment::Error>> {
    Ok(Figment::new()
        .merge(Serialized::defaults(AppConfig::default()))
        .merge(Toml::file(config_path()))
        .merge(Env::prefixed("SEARCH_").split("_"))
        .extract()?)
}

pub fn mask_key(key: &str) -> String {
    if key.len() <= 8 {
        if key.is_empty() {
            "(not set)".to_string()
        } else {
            format!("{}***", &key[..2])
        }
    } else {
        format!("{}...{}", &key[..4], &key[key.len() - 4..])
    }
}

pub fn config_show(config: &AppConfig) {
    use owo_colors::OwoColorize;
    use std::io::IsTerminal;
    let c = std::io::stdout().is_terminal();

    if c {
        println!("\n{}  Configuration\n", "search".bold().cyan());
        println!("  {} {}\n", "path:".dimmed(), config_path().display().to_string().dimmed());
    } else {
        println!("Configuration ({})\n", config_path().display());
    }

    use crate::providers;

    let keys: &[(&str, &str, &str)] = &[
        ("parallel",   &config.keys.parallel,   "PARALLEL_API_KEY"),
        ("brave",      &config.keys.brave,      "BRAVE_API_KEY"),
        ("serper",     &config.keys.serper,      "SERPER_API_KEY"),
        ("exa",        &config.keys.exa,         "EXA_API_KEY"),
        ("jina",       &config.keys.jina,        "JINA_API_KEY"),
        ("firecrawl",  &config.keys.firecrawl,   "FIRECRAWL_API_KEY"),
        ("tavily",     &config.keys.tavily,      "TAVILY_API_KEY"),
        ("serpapi",    &config.keys.serpapi,      "SERPAPI_API_KEY"),
        ("perplexity", &config.keys.perplexity,  "PERPLEXITY_API_KEY"),
        ("browserless",&config.keys.browserless,  "BROWSERLESS_API_KEY"),
        ("xai",        &config.keys.xai,         "XAI_API_KEY"),
        ("you",        &config.keys.you,         "YOU_API_KEY"),
    ];

    if c { println!("  {}", "[keys]".bold()); } else { println!("[keys]"); }
    for (name, config_val, env_var) in keys {
        let effective = providers::resolve_key(config_val, env_var);
        let masked = mask_key(&effective);
        if c {
            let val = if effective.is_empty() {
                masked.red().to_string()
            } else {
                masked.green().to_string()
            };
            println!("    {:<12} {}", name.white(), val);
        } else {
            println!("  {:<12} = {}", name, masked);
        }
    }

    println!();
    if c { println!("  {}", "[settings]".bold()); } else { println!("[settings]"); }
    if c {
        println!("    {:<10} {}", "timeout".white(), format!("{}s", config.settings.timeout).cyan());
        println!("    {:<10} {}", "count".white(), config.settings.count.to_string().cyan());
    } else {
        println!("  timeout  = {}s", config.settings.timeout);
        println!("  count    = {}", config.settings.count);
    }
    println!();
}

pub fn config_set(key: &str, value: &str) -> Result<(), crate::errors::SearchError> {
    let path = config_path();
    let mut doc: toml::Table = if path.exists() {
        let content = std::fs::read_to_string(&path)?;
        content
            .parse()
            .map_err(|e: toml::de::Error| crate::errors::SearchError::Config(e.to_string()))?
    } else {
        toml::Table::new()
    };

    // Support dotted keys: keys.brave, settings.timeout
    let parts: Vec<&str> = key.split('.').collect();
    match parts.len() {
        1 => {
            // Top-level keys are strings by convention (e.g., keys.*)
            doc.insert(parts[0].to_string(), toml::Value::String(value.to_string()));
        }
        2 => {
            let section = doc
                .entry(parts[0])
                .or_insert_with(|| toml::Value::Table(toml::Table::new()));
            if let toml::Value::Table(t) = section {
                // Typed handling for settings.* fields
                if parts[0] == "settings" {
                    match parts[1] {
                        "timeout" => {
                            // timeout is u64 in AppConfig; validate and store as integer
                            match value.parse::<u64>() {
                                Ok(vu) => {
                                    if vu <= i64::MAX as u64 {
                                        t.insert(parts[1].to_string(), toml::Value::Integer(vu as i64));
                                    } else {
                                        return Err(crate::errors::SearchError::Config(format!(
                                            "Value for {key} is too large"
                                        )));
                                    }
                                }
                                Err(_) => {
                                    return Err(crate::errors::SearchError::Config(format!(
                                        "Invalid numeric value for {key}: {value}"
                                    )));
                                }
                            }
                        }
                        "count" => {
                            // count is usize in AppConfig; validate and store as integer
                            match value.parse::<usize>() {
                                Ok(vc) => {
                                    // Convert usize -> i64 safely
                                    let vi = i64::try_from(vc).map_err(|_| {
                                        crate::errors::SearchError::Config(format!(
                                            "Value for {key} is too large"
                                        ))
                                    })?;
                                    t.insert(parts[1].to_string(), toml::Value::Integer(vi));
                                }
                                Err(_) => {
                                    return Err(crate::errors::SearchError::Config(format!(
                                        "Invalid numeric value for {key}: {value}"
                                    )));
                                }
                            }
                        }
                        _ => {
                            // Unknown setting — store as string to be conservative
                            t.insert(parts[1].to_string(), toml::Value::String(value.to_string()));
                        }
                    }
                } else {
                    // Other sections: store values as strings by default
                    t.insert(parts[1].to_string(), toml::Value::String(value.to_string()));
                }
            }
        }
        _ => {
            return Err(crate::errors::SearchError::Config(format!(
                "Invalid key: {key}"
            )));
        }
    }

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, doc.to_string())?;
    Ok(())
}

pub fn config_check(config: &AppConfig) {
    use owo_colors::OwoColorize;
    use std::io::IsTerminal;
    let c = std::io::stdout().is_terminal();

    use crate::providers;

    let all: &[(&str, &str, &str, &str)] = &[
        ("parallel",    &config.keys.parallel,    "PARALLEL_API_KEY",    "Independent web index (Parallel AI)"),
        ("brave",       &config.keys.brave,       "BRAVE_API_KEY",       "Web + News search"),
        ("serper",      &config.keys.serper,       "SERPER_API_KEY",      "Google SERP, Scholar, Patents, Images, Places"),
        ("exa",         &config.keys.exa,          "EXA_API_KEY",         "Semantic search, People, Similar pages"),
        ("jina",        &config.keys.jina,         "JINA_API_KEY",        "Web search + URL reader"),
        ("firecrawl",   &config.keys.firecrawl,    "FIRECRAWL_API_KEY",   "Web scraping + extraction"),
        ("tavily",      &config.keys.tavily,       "TAVILY_API_KEY",      "General, News, Academic, Deep search"),
        ("serpapi",     &config.keys.serpapi,       "SERPAPI_API_KEY",     "80+ engines: Google, Bing, YouTube, Baidu, Scholar"),
        ("perplexity",  &config.keys.perplexity,   "PERPLEXITY_API_KEY",  "AI-powered answers with citations (Perplexity Sonar)"),
        ("browserless", &config.keys.browserless,   "BROWSERLESS_API_KEY", "Cloud browser for Cloudflare/JS-heavy pages"),
        ("xai",         &config.keys.xai,          "XAI_API_KEY",         "X/Twitter social search via xAI Grok"),
        ("you",         &config.keys.you,          "YOU_API_KEY",         "LLM-ready web and news search"),
    ];

    if c {
        println!("\n{}  Provider Health Check\n", "search".bold().cyan());
    }

    let mut configured = 0;
    for (name, config_val, env_var, desc) in all {
        let is_configured = !providers::resolve_key(config_val, env_var).is_empty();
        if !is_configured {
            if c {
                println!("  {} {:<12} {}", "x".red().bold(), name.white(), desc.dimmed());
            } else {
                println!("  [x] {name}: NOT SET - {desc}");
            }
        } else {
            configured += 1;
            if c {
                println!("  {} {:<12} {}", "+".green().bold(), name.white().bold(), desc.dimmed());
            } else {
                println!("  [+] {name}: OK - {desc}");
            }
        }
    }

    println!();
    if configured == 0 {
        if c {
            println!("  {} No providers configured.\n", "!".yellow().bold());
            println!("  Set API keys via environment or config:");
            println!("    {} export BRAVE_API_KEY=YOUR_KEY", "$".dimmed());
            println!("    {} search config set keys.brave YOUR_KEY", "$".dimmed());
        } else {
            println!("  No providers configured. Set API keys via:");
            println!("    export BRAVE_API_KEY=<YOUR_KEY>");
            println!("    search config set keys.brave <YOUR_KEY>");
        }
    } else if c {
        println!(
            "  {}/{} providers ready",
            configured.to_string().green().bold(),
            all.len()
        );
    } else {
        println!("  {configured}/{} providers configured", all.len());
    }
    println!();
}
