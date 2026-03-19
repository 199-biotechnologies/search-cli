use crate::types::Mode;
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "search",
    version,
    about = "Agent-friendly multi-provider search CLI"
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
}

#[derive(Subcommand)]
pub enum Commands {
    /// Search across providers
    Search(SearchArgs),

    /// Manage configuration
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },

    /// Show machine-readable capabilities (for agents)
    AgentInfo,

    /// List all providers and their status
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

    /// Search mode
    #[arg(short, long, value_enum, default_value = "auto")]
    pub mode: Mode,

    /// Number of results
    #[arg(short, long)]
    pub count: Option<usize>,
}

#[derive(Subcommand)]
pub enum ConfigAction {
    /// Show current configuration (keys masked)
    Show,
    /// Set a configuration value
    Set {
        /// Config key (e.g. keys.brave, settings.timeout)
        key: String,
        /// Value to set
        value: String,
    },
    /// Check which providers are configured
    Check,
}
