use crate::types::Mode;
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "search",
    version,
    about = "Agent-friendly multi-provider search CLI",
    long_about = "Aggregates 11 search providers with 14 search modes.\n\
        Auto-detects intent from your query and routes to the best providers.\n\
        Outputs colored tables for humans, JSON when piped to other tools.\n\n\
        PROVIDERS:\n  \
          brave      Independent web index (35B pages), news search\n  \
          serper     Google SERP: web, news, scholar, patents, images, places\n  \
          exa        Neural/semantic search, LinkedIn people, find-similar\n  \
          jina       Fast web search + URL-to-markdown reader\n  \
          firecrawl  JS-rendered page scraping + structured extraction\n  \
          tavily     General, news, academic, deep search\n  \
          serpapi    80+ engines: Google, Bing, YouTube, Baidu, Scholar\n  \
          perplexity AI-powered answers with citations (Sonar)\n  \
          browserless Cloud browser for Cloudflare/JS-heavy pages\n  \
          stealth    Anti-bot stealth scraper\n  \
          xai        X/Twitter social search via xAI Grok\n\n\
        EXAMPLES:\n  \
          search \"rust error handling\"                    # auto-detect mode\n  \
          search search -q \"CRISPR\" -m academic           # academic papers\n  \
          search search -q \"CEO of Stripe\" -m people      # LinkedIn profiles via Exa\n  \
          search search -q \"AI news\" -m news              # breaking news\n  \
          search search -q \"trending on twitter\" -m social # X/Twitter search\n  \
          search search -q \"query\" -p exa                 # force Exa only\n  \
          search search -q \"query\" -p exa,brave           # only Exa + Brave\n  \
          search --x \"AI agents\"                          # search X (Twitter) only\n  \
          search \"query\" --json | jq '.results[].url'     # pipe JSON to jq"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,

    /// Search query (shorthand for `search -q`)
    #[arg(trailing_var_arg = true, global = false)]
    pub query_words: Vec<String>,

    /// Output as JSON (auto-enabled when piped)
    #[arg(long, global = true)]
    pub json: bool,

    /// Suppress non-essential output
    #[arg(long, global = true)]
    pub quiet: bool,

    /// Replay the last search result from cache
    #[arg(long, global = true)]
    pub last: bool,

    /// Search X (Twitter) only — shorthand for -m social -p xai
    #[arg(long = "x", global = true)]
    pub x_only: bool,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Search across providers (use -m for mode, -p to pick providers)
    Search(SearchArgs),

    /// Manage configuration (show, set, check)
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },

    /// Show machine-readable capabilities (for agents)
    AgentInfo,

    /// List all providers with status and capabilities
    Providers,

    /// Verify if email addresses exist via SMTP (no API key needed)
    Verify(VerifyArgs),

    /// Manage skill file installation for agent platforms
    Skill {
        #[command(subcommand)]
        action: SkillAction,
    },

    /// Check for updates or self-update
    Update {
        /// Only check, don't install
        #[arg(long)]
        check: bool,
    },
}

#[derive(Parser)]
pub struct VerifyArgs {
    /// Email addresses to verify
    pub emails: Vec<String>,

    /// Read emails from file (one per line, use - for stdin)
    #[arg(short, long)]
    pub file: Option<String>,
}

#[derive(Parser)]
pub struct SearchArgs {
    /// Search query
    #[arg(short, long)]
    pub query: String,

    /// Search mode [auto detects from query]
    #[arg(short, long, value_enum, default_value = "auto")]
    pub mode: Mode,

    /// Number of results to return
    #[arg(short, long)]
    pub count: Option<usize>,

    /// Use only specific providers (comma-separated: brave,serper,exa,jina,firecrawl,tavily,serpapi,perplexity,browserless,stealth,xai)
    #[arg(short, long, value_delimiter = ',')]
    pub providers: Option<Vec<String>>,

    /// Include only results from these domains (comma-separated)
    #[arg(short, long, value_delimiter = ',')]
    pub domain: Option<Vec<String>>,

    /// Exclude results from these domains (comma-separated)
    #[arg(long, value_delimiter = ',')]
    pub exclude_domain: Option<Vec<String>>,

    /// Freshness filter: day, week, month, year
    #[arg(short, long)]
    pub freshness: Option<String>,
}

#[derive(Subcommand)]
pub enum ConfigAction {
    /// Show current configuration (API keys masked)
    Show,
    /// Set a configuration value (e.g. keys.brave YOUR_KEY)
    Set {
        /// Config key (e.g. keys.brave, settings.timeout)
        key: String,
        /// Value to set
        value: String,
    },
    /// Health-check which providers are configured and ready
    Check,
    /// Show configuration file path
    Path,
}

#[derive(Subcommand)]
pub enum SkillAction {
    /// Write skill file to all detected agent platforms
    Install,
    /// Check which platforms have the skill installed
    Status,
}

pub mod skill {
    use crate::output::Ctx;
    use std::path::PathBuf;

    const SKILL_CONTENT: &str = include_str!("../SKILL.md");

    struct Target {
        name: &'static str,
        path: PathBuf,
    }

    fn home() -> PathBuf {
        std::env::var("HOME")
            .or_else(|_| std::env::var("USERPROFILE"))
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("."))
    }

    fn targets() -> Vec<Target> {
        let h = home();
        vec![
            Target { name: "Claude Code", path: h.join(".claude/skills/search") },
            Target { name: "Codex CLI", path: h.join(".codex/skills/search") },
            Target { name: "Gemini CLI", path: h.join(".gemini/skills/search") },
        ]
    }

    pub fn install(ctx: &Ctx) {
        let mut results = Vec::new();
        for t in &targets() {
            let skill_path = t.path.join("SKILL.md");
            let status = if skill_path.exists()
                && std::fs::read_to_string(&skill_path).is_ok_and(|c| c == SKILL_CONTENT)
            {
                "already_current"
            } else {
                if let Err(e) = std::fs::create_dir_all(&t.path) {
                    eprintln!("  Failed {}: {e}", t.name);
                    continue;
                }
                if let Err(e) = std::fs::write(&skill_path, SKILL_CONTENT) {
                    eprintln!("  Failed {}: {e}", t.name);
                    continue;
                }
                "installed"
            };
            results.push((t.name, skill_path.display().to_string(), status));
        }

        if ctx.is_json() {
            let items: Vec<serde_json::Value> = results
                .iter()
                .map(|(name, path, status)| {
                    serde_json::json!({"platform": name, "path": path, "status": status})
                })
                .collect();
            crate::output::json::render_value(&serde_json::json!({
                "version": "1",
                "status": "success",
                "data": items,
            }));
        } else if !ctx.suppress_human() {
            use owo_colors::OwoColorize;
            for (name, path, status) in &results {
                let marker = if *status == "installed" { "+" } else { "=" };
                println!(" {} {} -> {}", marker.green(), name.bold(), path.dimmed());
            }
        }
    }

    pub fn status(ctx: &Ctx) {
        let mut results = Vec::new();
        for t in &targets() {
            let skill_path = t.path.join("SKILL.md");
            let (installed, current) = if skill_path.exists() {
                let current =
                    std::fs::read_to_string(&skill_path).is_ok_and(|c| c == SKILL_CONTENT);
                (true, current)
            } else {
                (false, false)
            };
            results.push((t.name, installed, current));
        }

        if ctx.is_json() {
            let items: Vec<serde_json::Value> = results
                .iter()
                .map(|(name, installed, current)| {
                    serde_json::json!({"platform": name, "installed": installed, "current": current})
                })
                .collect();
            crate::output::json::render_value(&serde_json::json!({
                "version": "1",
                "status": "success",
                "data": items,
            }));
        } else if !ctx.suppress_human() {
            use owo_colors::OwoColorize;
            for (name, installed, current) in &results {
                let status = if *current {
                    "current".green().to_string()
                } else if *installed {
                    "outdated".yellow().to_string()
                } else {
                    "not installed".red().to_string()
                };
                println!("  {} {}", name.bold(), status);
            }
        }
    }
}
