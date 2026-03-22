use directories::ProjectDirs;
use figment::{
    providers::{Env, Format, Serialized, Toml},
    Figment,
};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub keys: ApiKeys,
    pub settings: Settings,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiKeys {
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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    #[serde(default = "default_timeout")]
    pub timeout: u64,
    #[serde(default = "default_count")]
    pub count: usize,
}

fn default_timeout() -> u64 {
    10
}
fn default_count() -> usize {
    10
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            keys: ApiKeys {
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
            },
            settings: Settings {
                timeout: default_timeout(),
                count: default_count(),
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

fn dirs_fallback() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    PathBuf::from(home).join(".config").join("search")
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

    let keys = [
        ("brave", &config.keys.brave),
        ("serper", &config.keys.serper),
        ("exa", &config.keys.exa),
        ("jina", &config.keys.jina),
        ("firecrawl", &config.keys.firecrawl),
        ("tavily", &config.keys.tavily),
        ("serpapi", &config.keys.serpapi),
        ("perplexity", &config.keys.perplexity),
        ("browserless", &config.keys.browserless),
        ("xai", &config.keys.xai),
    ];

    if c { println!("  {}", "[keys]".bold()); } else { println!("[keys]"); }
    for (name, key) in &keys {
        let masked = mask_key(key);
        if c {
            let val = if key.is_empty() {
                masked.red().to_string()
            } else {
                masked.green().to_string()
            };
            println!("    {:<10} {}", name.white(), val);
        } else {
            println!("  {:<10} = {}", name, masked);
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
            doc.insert(parts[0].to_string(), toml::Value::String(value.to_string()));
        }
        2 => {
            let section = doc
                .entry(parts[0])
                .or_insert_with(|| toml::Value::Table(toml::Table::new()));
            if let toml::Value::Table(t) = section {
                t.insert(parts[1].to_string(), toml::Value::String(value.to_string()));
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
    println!("Set {key} in {}", path.display());
    Ok(())
}

pub fn config_check(config: &AppConfig) {
    use owo_colors::OwoColorize;
    use std::io::IsTerminal;
    let c = std::io::stdout().is_terminal();

    let providers = [
        ("brave", &config.keys.brave, "Web + News search"),
        ("serper", &config.keys.serper, "Google SERP, Scholar, Patents, Images, Places"),
        ("exa", &config.keys.exa, "Semantic search, People, Similar pages"),
        ("jina", &config.keys.jina, "Web search + URL reader"),
        ("firecrawl", &config.keys.firecrawl, "Web scraping + extraction"),
        ("tavily", &config.keys.tavily, "General, News, Academic, Deep search"),
        ("serpapi", &config.keys.serpapi, "80+ engines: Google, Bing, YouTube, Baidu, Scholar"),
        ("perplexity", &config.keys.perplexity, "AI-powered answers with citations (Perplexity Sonar)"),
        ("browserless", &config.keys.browserless, "Cloud browser for Cloudflare/JS-heavy pages"),
        ("xai", &config.keys.xai, "X/Twitter social search via xAI Grok"),
    ];

    if c {
        println!("\n{}  Provider Health Check\n", "search".bold().cyan());
    }

    let mut configured = 0;
    for (name, key, desc) in &providers {
        if key.is_empty() {
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
            println!("  Set API keys via config file or environment:");
            println!("    {} search config set keys.brave YOUR_KEY", "$".dimmed());
            println!("    {} export SEARCH_KEYS_BRAVE=YOUR_KEY", "$".dimmed());
        } else {
            println!("  No providers configured. Set API keys via:");
            println!("    search config set keys.brave <YOUR_KEY>");
            println!("    export SEARCH_KEYS_BRAVE=<YOUR_KEY>");
        }
    } else if c {
        println!(
            "  {}/{} providers ready",
            configured.to_string().green().bold(),
            providers.len()
        );
    } else {
        println!("  {configured}/{} providers configured", providers.len());
    }
    println!();
}
