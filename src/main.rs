mod cache;
mod classify;
mod cli;
mod config;
mod context;
mod engine;
mod errors;
mod logging;
mod output;
mod providers;
mod types;
mod utils;
mod verify;

use clap::Parser;
use cli::{Cli, Commands, ConfigAction, SkillAction};
use config::{config_check, config_set, config_show, load_config};
use context::AppContext;
use output::{Ctx, OutputFormat};
use std::sync::Arc;
use tokio::net::lookup_host;
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

/// Pre-scan argv for --json before clap parses. Ensures --json works on
/// help, version, and parse-error paths where Cli hasn't been populated.
fn has_json_flag() -> bool {
    let mut past_dashdash = false;
    for arg in std::env::args_os().skip(1) {
        if arg == "--" {
            past_dashdash = true;
        }
        if !past_dashdash && arg == "--json" {
            return true;
        }
    }
    false
}

fn init_tracing() {
    // Quiet by default unless caller explicitly opts in.
    let rust_log = std::env::var("RUST_LOG").unwrap_or_default();
    if rust_log.trim().is_empty() {
        return;
    }

    let filter = EnvFilter::try_new(rust_log).unwrap_or_else(|_| EnvFilter::new("info"));
    let fmt_layer = fmt::layer()
        .with_target(false)
        .with_thread_ids(false)
        .with_thread_names(false)
        .without_time()
        .with_ansi(false)
        .with_writer(std::io::stderr);

    let _ = tracing_subscriber::registry()
        .with(filter)
        .with(fmt_layer)
        .try_init();
}

#[tokio::main]
async fn main() {
    init_tracing();
    crate::cache::evict_expired();

    // 1. Pre-emptive DNS resolution (starts immediately in background)
    tokio::spawn(async {
        let domains = [
            "api.parallel.ai:443",
            "api.search.brave.com:443",
            "google.serper.dev:443",
            "api.exa.ai:443",
            "api.jina.ai:443",
            "api.tavily.com:443",
            "api.perplexity.ai:443",
        ];
        for domain in domains {
            let _ = lookup_host(domain).await;
        }
    });

    // 2. Start loading config in parallel with CLI parsing
    let config_handle = tokio::task::spawn_blocking(load_config);

    // 3. Pre-scan --json before clap parses
    let json_flag = has_json_flag();

    // 4. CLI Parsing — use try_parse so we own error handling
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(e) => {
            if matches!(
                e.kind(),
                clap::error::ErrorKind::DisplayHelp
                    | clap::error::ErrorKind::DisplayVersion
            ) {
                let format = OutputFormat::detect(json_flag);
                match format {
                    OutputFormat::Json => {
                        let envelope = serde_json::json!({
                            "version": "1",
                            "status": "success",
                            "data": { "usage": e.to_string().trim_end() },
                        });
                        println!(
                            "{}",
                            serde_json::to_string_pretty(&envelope).unwrap()
                        );
                        std::process::exit(0);
                    }
                    OutputFormat::Table => e.exit(),
                }
            }

            // Parse errors — we own the exit code, always 3.
            let format = OutputFormat::detect(json_flag);
            match format {
                OutputFormat::Json => {
                    let envelope = serde_json::json!({
                        "version": "1",
                        "status": "error",
                        "error": {
                            "code": "invalid_input",
                            "message": e.to_string(),
                            "suggestion": "Check arguments with: search --help",
                        },
                    });
                    eprintln!(
                        "{}",
                        serde_json::to_string_pretty(&envelope).unwrap()
                    );
                }
                OutputFormat::Table => {
                    eprint!("{e}");
                }
            }
            std::process::exit(3);
        }
    };

    let ctx = Ctx::new(cli.json, cli.quiet);

    // 5. Wait for config
    let config = match config_handle.await.unwrap() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Config error: {e}");
            std::process::exit(1);
        }
    };

    let app = match AppContext::new(config) {
        Ok(ctx) => Arc::new(ctx),
        Err(e) => {
            eprintln!("Failed to initialize app: {e}");
            std::process::exit(1);
        }
    };
    tracing::info!(event = "app_initialized", timeout_s = app.config.settings.timeout, default_count = app.config.settings.count);

    // 6. Pre-emptive TLS Handshake
    let is_search = cli.command.is_none() || matches!(cli.command, Some(Commands::Search(_)));
    if is_search && !cli.last {
        let app_c = app.clone();
        tokio::spawn(async move {
            let urls = [
                "https://api.search.brave.com/res/v1/web/search",
                "https://google.serper.dev/search",
                "https://api.exa.ai/search",
            ];
            for url in urls {
                let _ = app_c.client.head(url).send().await;
            }
        });
    }

    let exit_code = match run(cli, &ctx, app).await {
        Ok(code) => code,
        Err(e) => {
            tracing::warn!(event = "search_failed", code = e.error_code(), message = %e);
            if ctx.is_json() {
                output::json::render_error(&e);
            } else {
                eprintln!("Error: {e}");
            }
            e.exit_code()
        }
    };

    std::process::exit(exit_code);
}

async fn run(cli: Cli, ctx: &Ctx, app: Arc<AppContext>) -> Result<i32, errors::SearchError> {
    // Handle bare `search "query"` without subcommand
    let command = if let Some(cmd) = cli.command {
        cmd
    } else if cli.last {
        Commands::Search(cli::SearchArgs {
            query: String::new(),
            mode: types::Mode::Auto,
            count: None,
            providers: None,
            domain: None,
            exclude_domain: None,
            freshness: None,
        })
    } else if !cli.query_words.is_empty() {
        let query = cli.query_words.join(" ");
        Commands::Search(cli::SearchArgs {
            query,
            mode: types::Mode::Auto,
            count: None,
            providers: None,
            domain: None,
            exclude_domain: None,
            freshness: None,
        })
    } else {
        use clap::CommandFactory;
        if ctx.is_json() {
            let mut buf = Vec::new();
            Cli::command().write_long_help(&mut buf).ok();
            let envelope = serde_json::json!({
                "version": "1",
                "status": "success",
                "data": { "usage": String::from_utf8_lossy(&buf).trim_end() },
            });
            println!("{}", serde_json::to_string_pretty(&envelope).unwrap());
        } else {
            Cli::command().print_help().ok();
            println!();
        }
        return Ok(0);
    };

    match command {
        Commands::Search(mut args) => {
            // --x flag: force X/Twitter search via xAI Grok
            if cli.x_only {
                args.mode = types::Mode::Social;
                args.providers = Some(vec!["xai".to_string()]);
            }

            if cli.last {
                if let Some(cached) = cache::load_last() {
                    if ctx.is_json() {
                        output::json::render(&cached);
                    } else if !ctx.suppress_human() {
                        output::table::render(&cached);
                    }
                    return Ok(0);
                } else {
                    let err = errors::SearchError::Config("No cached results found. Run a search first.".into());
                    tracing::warn!(event = "search_failed", code = err.error_code(), message = %err);
                    if ctx.is_json() {
                        output::json::render_error(&err);
                    } else {
                        eprintln!("No cached results found. Run a search first.");
                    }
                    return Ok(1);
                }
            }

            // Validate provider names early
            if let Some(ref providers) = args.providers {
                const KNOWN: &[&str] = &[
                    "parallel", "brave", "serper", "exa", "jina", "firecrawl", "tavily",
                    "serpapi", "perplexity", "browserless", "stealth", "xai", "you",
                ];
                for p in providers {
                    if !KNOWN.iter().any(|k| k.eq_ignore_ascii_case(p)) {
                        let err = errors::SearchError::Config(format!(
                            "Unknown provider '{}'. Valid: {}", p, KNOWN.join(", ")
                        ));
                        tracing::warn!(event = "search_failed", code = err.error_code(), message = %err);
                        if ctx.is_json() {
                            output::json::render_error(&err);
                        } else {
                            eprintln!("Error: {err}");
                        }
                        return Ok(err.exit_code());
                    }
                }
            }

            let count = args.count.unwrap_or(app.config.settings.count);
            let opts = types::SearchOpts {
                include_domains: args.domain.unwrap_or_default(),
                exclude_domains: args.exclude_domain.unwrap_or_default(),
                freshness: args.freshness,
            };

            // Check query cache (5min TTL)
            let mode_str = args.mode.to_string();
            if args.providers.is_none()
                && opts.include_domains.is_empty()
                && opts.exclude_domains.is_empty()
                && opts.freshness.is_none()
            {
                if let Some(cached) = cache::load_query(&args.query, &mode_str) {
                    if ctx.is_json() {
                        output::json::render(&cached);
                    } else if !ctx.suppress_human() {
                        output::table::render(&cached);
                    }
                    return Ok(0);
                }
            }

            // Show spinner for human output (suppressed by --quiet)
            let spinner = if !ctx.is_json() && !ctx.quiet {
                let sp = indicatif::ProgressBar::new_spinner();
                sp.set_style(
                    indicatif::ProgressStyle::default_spinner()
                        .tick_strings(&["   ", ".  ", ".. ", "...", " ..", "  .", "   "])
                        .template("  {spinner:.cyan} searching {msg}")
                        .unwrap(),
                );
                let provider_hint = args
                    .providers
                    .as_ref()
                    .map(|p| format!(" via {}", p.join(", ")))
                    .unwrap_or_default();
                sp.set_message(format!(
                    "\"{}\" [{}{}]",
                    args.query,
                    args.mode,
                    provider_hint
                ));
                sp.enable_steady_tick(std::time::Duration::from_millis(100));
                Some(sp)
            } else {
                None
            };

            let response =
                engine::run(app, &args.query, args.mode, count, &args.providers, &opts).await;

            if let Some(sp) = spinner {
                sp.finish_and_clear();
            }

            let response = response?;

            tracing::info!(
                event = "search_completed",
                mode = %response.mode,
                status = %response.status,
                elapsed_ms = response.metadata.elapsed_ms,
                result_count = response.metadata.result_count,
                providers_queried = ?response.metadata.providers_queried,
                providers_failed = ?response.metadata.providers_failed
            );

            cache::save_last(&response);
            cache::save_query(&args.query, &mode_str, &response);
            logging::log_search(&response);

            if ctx.is_json() {
                output::json::render(&response);
            } else if !ctx.suppress_human() {
                output::table::render(&response);
            }

            if response.status == "all_providers_failed" {
                Ok(1)
            } else {
                Ok(0)
            }
        }

        Commands::Config { action } => {
            match action {
                ConfigAction::Show => {
                    if ctx.is_json() {
                        let configured: Vec<&str> = [
                            ("brave", !app.config.keys.brave.is_empty()),
                            ("serper", !app.config.keys.serper.is_empty()),
                            ("exa", !app.config.keys.exa.is_empty()),
                            ("jina", !app.config.keys.jina.is_empty()),
                            ("firecrawl", !app.config.keys.firecrawl.is_empty()),
                            ("tavily", !app.config.keys.tavily.is_empty()),
                            ("serpapi", !app.config.keys.serpapi.is_empty()),
                            ("perplexity", !app.config.keys.perplexity.is_empty()),
                            ("browserless", !app.config.keys.browserless.is_empty()),
                            ("xai", !app.config.keys.xai.is_empty()),
                            ("you", !app.config.keys.you.is_empty()),
                        ].iter().filter(|(_, v)| *v).map(|(k, _)| *k).collect();
                        let info = serde_json::json!({
                            "version": "1",
                            "status": "success",
                            "config_path": config::config_path().to_string_lossy(),
                            "settings": {
                                "timeout": app.config.settings.timeout,
                                "count": app.config.settings.count,
                            },
                            "providers_configured": configured,
                        });
                        output::json::render_value(&info);
                    } else if !ctx.suppress_human() {
                        config_show(&app.config);
                    }
                }
                ConfigAction::Set { key, value } => {
                    config_set(&key, &value)?;
                    if ctx.is_json() {
                        output::json::render_value(&serde_json::json!({
                            "version": "1",
                            "status": "success",
                            "key": key,
                            "message": format!("Set {key}"),
                        }));
                    } else if !ctx.suppress_human() {
                        eprintln!("Set {key}");
                    }
                }
                ConfigAction::Check => {
                    if ctx.is_json() {
                        let all_providers = providers::build_providers(&app);
                        let all: Vec<(&str, bool)> = all_providers
                            .iter()
                            .map(|p| (p.name(), p.is_configured()))
                            .collect();
                        let configured: Vec<&str> = all.iter().filter(|(_, v)| *v).map(|(k, _)| *k).collect();
                        let unconfigured: Vec<&str> = all.iter().filter(|(_, v)| !v).map(|(k, _)| *k).collect();
                        let total = all.len();
                        output::json::render_value(&serde_json::json!({
                            "version": "1",
                            "status": "success",
                            "configured_count": configured.len(),
                            "total_count": total,
                            "configured": configured,
                            "unconfigured": unconfigured,
                        }));
                    } else if !ctx.suppress_human() {
                        config_check(&app.config);
                    }
                }
                ConfigAction::Path => {
                    let p = config::config_path();
                    if ctx.is_json() {
                        output::json::render_value(&serde_json::json!({
                            "version": "1",
                            "status": "success",
                            "data": {
                                "path": p.to_string_lossy(),
                                "exists": p.exists(),
                            },
                        }));
                    } else if !ctx.suppress_human() {
                        println!("{}", p.display());
                        if !p.exists() {
                            use owo_colors::OwoColorize;
                            println!("  {}", "(file does not exist, using defaults)".dimmed());
                        }
                    }
                }
            }
            Ok(0)
        }

        Commands::AgentInfo => {
            let all = providers::build_providers(&app);
            let providers_info: Vec<serde_json::Value> = all
                .iter()
                .map(|p| {
                    serde_json::json!({
                        "name": p.name(),
                        "configured": p.is_configured(),
                        "capabilities": p.capabilities(),
                        "env_keys": p.env_keys(),
                    })
                })
                .collect();

            let info = serde_json::json!({
                "name": "search",
                "version": env!("CARGO_PKG_VERSION"),
                "description": env!("CARGO_PKG_DESCRIPTION"),
                "commands": ["search", "verify", "config show", "config set", "config check", "config path", "agent-info", "providers", "skill install", "skill status", "update"],
                "command_schemas": {
                    "search": {
                        "description": "Search across providers",
                        "args": [
                            {"name": "-q/--query", "type": "string", "required": true, "description": "Search query"},
                        ],
                        "options": [
                            {"name": "-m/--mode", "type": "string", "required": false, "default": "auto",
                             "values": ["auto","general","news","academic","people","deep","extract","similar","scrape","scholar","patents","images","places","social"],
                             "description": "Search mode"},
                            {"name": "-c/--count", "type": "integer", "required": false, "description": "Number of results"},
                            {"name": "-p/--providers", "type": "string[]", "required": false,
                             "values": ["parallel","brave","serper","exa","jina","firecrawl","tavily","serpapi","perplexity","browserless","stealth","xai","you"],
                             "description": "Comma-separated provider list"},
                            {"name": "-d/--domain", "type": "string[]", "required": false, "description": "Include only these domains"},
                            {"name": "--exclude-domain", "type": "string[]", "required": false, "description": "Exclude these domains"},
                            {"name": "-f/--freshness", "type": "string", "required": false,
                             "values": ["day","week","month","year"],
                             "description": "Freshness filter"},
                        ]
                    },
                    "verify": {
                        "description": "Check if email addresses exist via SMTP",
                        "args": [
                            {"name": "emails", "type": "string[]", "required": false, "description": "Email addresses to verify"},
                        ],
                        "options": [
                            {"name": "-f/--file", "type": "string", "required": false, "description": "Read emails from file (use - for stdin)"},
                        ],
                        "verdicts": ["valid","invalid","catch_all","unreachable","timeout","syntax_error"],
                        "notes": "No API key required. Uses direct SMTP."
                    },
                    "config show": {"description": "Display current configuration (keys masked)", "args": [], "options": []},
                    "config set": {
                        "description": "Set a configuration value",
                        "args": [
                            {"name": "key", "type": "string", "required": true, "description": "Config key (e.g. keys.brave, settings.timeout)"},
                            {"name": "value", "type": "string", "required": true, "description": "Value to set"},
                        ],
                        "options": []
                    },
                    "config check": {"description": "Health-check which providers are configured", "args": [], "options": []},
                    "config path": {"description": "Show configuration file path", "args": [], "options": []},
                    "agent-info": {"description": "This manifest", "aliases": ["info"], "args": [], "options": []},
                    "providers": {"description": "List all providers with status and capabilities", "args": [], "options": []},
                    "skill install": {"description": "Install skill file to agent platforms", "args": [], "options": []},
                    "skill status": {"description": "Check skill installation status", "args": [], "options": []},
                    "update": {
                        "description": "Self-update binary from GitHub Releases",
                        "args": [],
                        "options": [
                            {"name": "--check", "type": "bool", "required": false, "default": false, "description": "Check only, don't install"}
                        ]
                    },
                },
                "global_flags": {
                    "--json": {"type": "bool", "default": false, "description": "Force JSON output (auto-enabled when piped)"},
                    "--quiet": {"type": "bool", "default": false, "description": "Suppress informational output"},
                    "--last": {"type": "bool", "default": false, "description": "Replay last search from cache"},
                    "--x": {"type": "bool", "default": false, "description": "Search X (Twitter) only"},
                },
                "exit_codes": {
                    "0": "Success",
                    "1": "Transient error (API, network) -- retry",
                    "2": "Config/auth error -- fix setup",
                    "3": "Bad input -- fix arguments",
                    "4": "Rate limited -- wait and retry",
                },
                "envelope": {
                    "version": "1",
                    "success": "{ version, status, data|results }",
                    "error": "{ version, status, error: { code, message, suggestion } }",
                },
                "providers": providers_info,
                "modes": ["auto","general","news","academic","people","deep","extract","similar","scrape","scholar","patents","images","places","social"],
                "config": {
                    "path": config::config_path().to_string_lossy(),
                    "env_prefix": "SEARCH_",
                },
                "auto_json_when_piped": true,
                "not_suited_for": {
                    "github_repos": {
                        "task": "Searching GitHub repositories, code, issues, or PRs",
                        "use_instead": "gh search repos <query> [--language=<lang>] [--sort=stars] [--json fullName,description,stargazersCount,url]",
                        "why": "search uses web crawl, not GitHub's API — no star counts, language filters, or structured repo metadata. gh queries GitHub's search API directly."
                    },
                    "github_code": {
                        "task": "Searching code inside GitHub repositories",
                        "use_instead": "gh search code <query> [--language=<lang>] [--json path,repository,textMatches]",
                        "why": "GitHub code search requires GitHub's index, not web search."
                    },
                    "github_issues": {
                        "task": "Searching GitHub issues or pull requests",
                        "use_instead": "gh search issues <query> [--state=open] [--json title,url,state] or gh search prs <query>",
                        "why": "GitHub issues/PRs require GitHub's API for state, labels, and metadata."
                    }
                },
            });

            output::json::render_value(&info);
            Ok(0)
        }

        Commands::Skill { action } => {
            match action {
                SkillAction::Install => cli::skill::install(ctx),
                SkillAction::Status => cli::skill::status(ctx),
            }
            Ok(0)
        }

        Commands::Providers => {
            let all = providers::build_providers(&app);
            let provider_info: Vec<(String, bool, Vec<String>)> = all
                .iter()
                .map(|p| {
                    (
                        p.name().to_string(),
                        p.is_configured(),
                        p.capabilities().iter().map(|s| s.to_string()).collect(),
                    )
                })
                .collect();

            if ctx.is_json() {
                let json: Vec<serde_json::Value> = provider_info
                    .iter()
                    .map(|(name, configured, caps)| {
                        serde_json::json!({
                            "name": name,
                            "configured": configured,
                            "capabilities": caps,
                        })
                    })
                    .collect();
                output::json::render_value(&serde_json::json!({
                    "version": "1",
                    "status": "success",
                    "providers": json,
                }));
            } else if !ctx.suppress_human() {
                output::table::render_providers(&provider_info);
            }
            Ok(0)
        }

        Commands::Verify(args) => {
            let mut emails: Vec<String> = args.emails;
            if let Some(ref path) = args.file {
                let content = if path == "-" {
                    use std::io::Read;
                    let mut buf = String::new();
                    std::io::stdin().read_to_string(&mut buf)?;
                    buf
                } else {
                    std::fs::read_to_string(path)?
                };
                emails.extend(
                    content.lines()
                        .map(|l| l.trim().to_string())
                        .filter(|l| !l.is_empty() && l.contains('@'))
                );
            }

            if emails.is_empty() {
                let err = errors::SearchError::Config(
                    "No email addresses provided. Usage: search verify user@example.com".into(),
                );
                if ctx.is_json() {
                    output::json::render_error(&err);
                } else {
                    eprintln!("Error: {err}");
                }
                return Ok(2);
            }

            let start = std::time::Instant::now();
            let results = match verify::verify_emails(&emails).await {
                Ok(r) => r,
                Err(e) => {
                    eprintln!("Error: {}", e);
                    return Ok(2);
                }
            };
            let elapsed = start.elapsed().as_millis();

            let valid_count = results.iter().filter(|r| r.verdict == "valid").count();
            let invalid_count = results.iter().filter(|r| r.verdict == "invalid").count();
            let catch_all_count = results.iter().filter(|r| r.verdict == "catch_all").count();

            let response = serde_json::json!({
                "version": "1",
                "status": "success",
                "results": results,
                "metadata": {
                    "elapsed_ms": elapsed,
                    "verified_count": results.len(),
                    "valid_count": valid_count,
                    "invalid_count": invalid_count,
                    "catch_all_count": catch_all_count,
                }
            });

            if ctx.is_json() {
                output::json::render_value(&response);
            } else if !ctx.suppress_human() {
                verify::render_table(&results);
            }

            Ok(0)
        }

        Commands::Update { check } => {
            let current = env!("CARGO_PKG_VERSION");
            if check {
                match self_update::backends::github::Update::configure()
                    .repo_owner("199-biotechnologies")
                    .repo_name("search-cli")
                    .bin_name("search")
                    .current_version(current)
                    .build()
                {
                    Ok(updater) => match updater.get_latest_release() {
                        Ok(release) => {
                            let up_to_date = release.version == current;
                            if ctx.is_json() {
                                output::json::render_value(&serde_json::json!({
                                    "version": "1",
                                    "status": "success",
                                    "current_version": current,
                                    "latest_version": release.version,
                                    "update_available": !up_to_date,
                                }));
                            } else if !ctx.suppress_human() {
                                if !up_to_date {
                                    eprintln!("Current version: {current}");
                                    eprintln!("New version available: {}", release.version);
                                    eprintln!("Run `search update` to install");
                                } else {
                                    eprintln!("Already up to date (v{current})");
                                }
                            }
                        }
                        Err(e) => {
                            if ctx.is_json() {
                                let err = errors::SearchError::Api {
                                    provider: "github",
                                    code: "update_check_failed",
                                    message: e.to_string(),
                                };
                                output::json::render_error(&err);
                            } else {
                                eprintln!("Could not check for updates: {e}");
                            }
                            return Ok(1);
                        }
                    },
                    Err(e) => {
                        if ctx.is_json() {
                            let err = errors::SearchError::Config(format!("Update check failed: {e}"));
                            output::json::render_error(&err);
                        } else {
                            eprintln!("Update check failed: {e}");
                        }
                        return Ok(1);
                    }
                }
            } else {
                if !ctx.suppress_human() {
                    eprintln!("Updating search from v{current}...");
                }
                match self_update::backends::github::Update::configure()
                    .repo_owner("199-biotechnologies")
                    .repo_name("search-cli")
                    .bin_name("search")
                    .current_version(current)
                    .build()
                    .and_then(|u| u.update())
                {
                    Ok(status) => {
                        if ctx.is_json() {
                            output::json::render_value(&serde_json::json!({
                                "version": "1",
                                "status": "success",
                                "updated": status.updated(),
                                "version_installed": status.version(),
                            }));
                        } else if !ctx.suppress_human() {
                            if status.updated() {
                                eprintln!("Updated to v{}", status.version());
                            } else {
                                eprintln!("Already up to date (v{current})");
                            }
                        }
                    }
                    Err(e) => {
                        if ctx.is_json() {
                            let err = errors::SearchError::Config(format!("Update failed: {e}"));
                            output::json::render_error(&err);
                        } else {
                            eprintln!("Update failed: {e}");
                            eprintln!("You can update manually: cargo install agent-search");
                        }
                        return Ok(1);
                    }
                }
            }
            Ok(0)
        }
    }
}
