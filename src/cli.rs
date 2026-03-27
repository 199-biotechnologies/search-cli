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

    /// Check for updates or self-update
    Update {
        /// Only check, don't install
        #[arg(long)]
        check: bool,
    },
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
}
