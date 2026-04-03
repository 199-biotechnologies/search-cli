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
mod verify;

use clap::Parser;
use cli::{Cli, Commands, ConfigAction};
use config::{config_check, config_set, config_show, load_config};
use context::AppContext;
use output::OutputFormat;
use std::sync::Arc;
use tokio::net::lookup_host;

#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

#[tokio::main]
async fn main() {
    // 1. Pre-emptive DNS resolution (starts immediately in background)
    // Priming the OS DNS cache for the most likely API domains.
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

    // 3. CLI Parsing (fast, but we want to overlap it with I/O)
    let cli = Cli::parse();
    let format = OutputFormat::detect(cli.json);

    // 4. Wait for config
    let config = match config_handle.await.unwrap() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Config error: {e}");
            std::process::exit(1);
        }
    };

    let ctx = Arc::new(AppContext::new(config));

    // 5. Pre-emptive TLS Handshake (Unconventional: Race to the wire)
    // If this is a search command, start Warming up TLS sessions for the "Big 3".
    let is_search = cli.command.is_none() || matches!(cli.command, Some(Commands::Search(_)));
    if is_search && !cli.last {
        let ctx_c = ctx.clone();
        tokio::spawn(async move {
            let urls = [
                "https://api.search.brave.com/res/v1/web/search",
                "https://google.serper.dev/search",
                "https://api.exa.ai/search",
            ];
            for url in urls {
                // Send a HEAD request to trigger TLS handshake + connection pooling.
                // We don't care about the result, just want the connection established.
                let _ = ctx_c.client.head(url).send().await;
            }
        });
    }

    let exit_code = match run(cli, &format, ctx).await {
        Ok(code) => code,
        Err(e) => {
            match format {
                OutputFormat::Json => output::json::render_error(&e),
                OutputFormat::Table => eprintln!("Error: {e}"),
            }
            e.exit_code()
        }
    };

    std::process::exit(exit_code);
}

async fn run(cli: Cli, format: &OutputFormat, ctx: Arc<AppContext>) -> Result<i32, errors::SearchError> {
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
        Cli::command().print_help().ok();
        println!();
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
                    match *format {
                        OutputFormat::Json => output::json::render(&cached),
                        OutputFormat::Table => output::table::render(&cached),
                    }
                    return Ok(0);
                } else {
                    let err = errors::SearchError::Config("No cached results found. Run a search first.".into());
                    match *format {
                        OutputFormat::Json => output::json::render_error(&err),
                        OutputFormat::Table => eprintln!("No cached results found. Run a search first."),
                    }
                    return Ok(1);
                }
            }

            // Validate provider names early
            if let Some(ref providers) = args.providers {
                const KNOWN: &[&str] = &[
                    "parallel", "brave", "serper", "exa", "jina", "firecrawl", "tavily",
                    "serpapi", "perplexity", "browserless", "stealth", "xai",
                ];
                for p in providers {
                    if !KNOWN.iter().any(|k| k.eq_ignore_ascii_case(p)) {
                        let err = errors::SearchError::Config(format!(
                            "Unknown provider '{}'. Valid: {}", p, KNOWN.join(", ")
                        ));
                        match *format {
                            OutputFormat::Json => output::json::render_error(&err),
                            OutputFormat::Table => eprintln!("Error: {err}"),
                        }
                        return Ok(err.exit_code());
                    }
                }
            }

            let count = args.count.unwrap_or(ctx.config.settings.count);
            let opts = types::SearchOpts {
                include_domains: args.domain.unwrap_or_default(),
                exclude_domains: args.exclude_domain.unwrap_or_default(),
                freshness: args.freshness,
            };

            // Check query cache (5min TTL) — skip if filters or provider selection is active
            let mode_str = args.mode.to_string();
            if args.providers.is_none()
                && opts.include_domains.is_empty()
                && opts.exclude_domains.is_empty()
                && opts.freshness.is_none()
            {
                if let Some(cached) = cache::load_query(&args.query, &mode_str) {
                    match *format {
                        OutputFormat::Json => output::json::render(&cached),
                        OutputFormat::Table => output::table::render(&cached),
                    }
                    return Ok(0);
                }
            }

            // Show spinner for human output
            let spinner = if matches!(*format, OutputFormat::Table) && !cli.quiet {
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
                engine::run(ctx, &args.query, args.mode, count, &args.providers, &opts).await;

            if let Some(sp) = spinner {
                sp.finish_and_clear();
            }

            let response = response?;

            cache::save_last(&response);
            cache::save_query(&args.query, &mode_str, &response);
            logging::log_search(&response);

            match *format {
                OutputFormat::Json => output::json::render(&response),
                OutputFormat::Table => output::table::render(&response),
            }

            // Exit non-zero when all providers failed (semantic exit codes)
            if response.status == "all_providers_failed" {
                Ok(1)
            } else {
                Ok(0)
            }
        }

        Commands::Config { action } => {
            match action {
                ConfigAction::Show => {
                    if matches!(*format, OutputFormat::Json) {
                        let configured: Vec<&str> = [
                            ("brave", !ctx.config.keys.brave.is_empty()),
                            ("serper", !ctx.config.keys.serper.is_empty()),
                            ("exa", !ctx.config.keys.exa.is_empty()),
                            ("jina", !ctx.config.keys.jina.is_empty()),
                            ("firecrawl", !ctx.config.keys.firecrawl.is_empty()),
                            ("tavily", !ctx.config.keys.tavily.is_empty()),
                            ("serpapi", !ctx.config.keys.serpapi.is_empty()),
                            ("perplexity", !ctx.config.keys.perplexity.is_empty()),
                            ("browserless", !ctx.config.keys.browserless.is_empty()),
                            ("xai", !ctx.config.keys.xai.is_empty()),
                        ].iter().filter(|(_, v)| *v).map(|(k, _)| *k).collect();
                        let info = serde_json::json!({
                            "version": "1",
                            "status": "success",
                            "config_path": config::config_path().to_string_lossy(),
                            "settings": {
                                "timeout": ctx.config.settings.timeout,
                                "count": ctx.config.settings.count,
                            },
                            "providers_configured": configured,
                        });
                        output::json::render_value(&info);
                    } else {
                        config_show(&ctx.config);
                    }
                }
                ConfigAction::Set { key, value } => {
                    config_set(&key, &value)?;
                    if matches!(*format, OutputFormat::Json) {
                        output::json::render_value(&serde_json::json!({
                            "version": "1",
                            "status": "success",
                            "key": key,
                            "message": format!("Set {key}"),
                        }));
                    } else {
                        eprintln!("Set {key}");
                    }
                }
                ConfigAction::Check => {
                    if matches!(*format, OutputFormat::Json) {
                        let all_providers = providers::build_providers(&ctx);
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
                    } else {
                        config_check(&ctx.config);
                    }
                }
            }
            Ok(0)
        }

        Commands::AgentInfo => {
            let all = providers::build_providers(&ctx);
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
                "commands": ["search", "verify", "config show", "config set", "config check", "agent-info", "providers", "update"],
                "modes": ["auto", "general", "news", "academic", "people", "deep", "extract", "similar", "scrape", "scholar", "patents", "images", "places", "social"],
                "providers": providers_info,
                "global_flags": ["--json", "--quiet", "--last", "--x"],
                "env_prefix": "SEARCH_",
                "config_path": config::config_path().to_string_lossy(),
                "output_formats": ["json", "table"],
                "auto_json_when_piped": true,
                "verify": {
                    "description": "Check if email addresses exist via SMTP without sending mail. No API key required.",
                    "usage": "search verify <email> [<email>...] [-f <file>] [--json]",
                    "verdicts": ["valid", "invalid", "catch_all", "unreachable", "timeout", "syntax_error"],
                    "examples": [
                        "search verify alice@stripe.com",
                        "search verify alice@stripe.com bob@gucci.com --json",
                        "search verify -f emails.txt"
                    ],
                    "notes": "No API key required. Uses direct SMTP. catch_all means domain accepts all addresses — email format likely correct but unverifiable. is_disposable flags throwaway email services."
                },
            });

            output::json::render_value(&info);
            Ok(0)
        }

        Commands::Providers => {
            let all = providers::build_providers(&ctx);
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

            match *format {
                OutputFormat::Json => {
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
                }
                OutputFormat::Table => {
                    output::table::render_providers(&provider_info);
                }
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
                match *format {
                    OutputFormat::Json => output::json::render_error(&err),
                    OutputFormat::Table => eprintln!("Error: {err}"),
                }
                return Ok(2);
            }

            let start = std::time::Instant::now();
            let results = verify::verify_emails(&emails).await;
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

            match *format {
                OutputFormat::Json => output::json::render_value(&response),
                OutputFormat::Table => verify::render_table(&results),
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
                            if matches!(*format, OutputFormat::Json) {
                                output::json::render_value(&serde_json::json!({
                                    "version": "1",
                                    "status": "success",
                                    "current_version": current,
                                    "latest_version": release.version,
                                    "update_available": !up_to_date,
                                }));
                            } else if !up_to_date {
                                eprintln!("Current version: {current}");
                                eprintln!("New version available: {}", release.version);
                                eprintln!("Run `search update` to install");
                            } else {
                                eprintln!("Already up to date (v{current})");
                            }
                        }
                        Err(e) => {
                            if matches!(*format, OutputFormat::Json) {
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
                        if matches!(*format, OutputFormat::Json) {
                            let err = errors::SearchError::Config(format!("Update check failed: {e}"));
                            output::json::render_error(&err);
                        } else {
                            eprintln!("Update check failed: {e}");
                        }
                        return Ok(1);
                    }
                }
            } else {
                eprintln!("Updating search from v{current}...");
                match self_update::backends::github::Update::configure()
                    .repo_owner("199-biotechnologies")
                    .repo_name("search-cli")
                    .bin_name("search")
                    .current_version(current)
                    .build()
                    .and_then(|u| u.update())
                {
                    Ok(status) => {
                        if matches!(*format, OutputFormat::Json) {
                            output::json::render_value(&serde_json::json!({
                                "version": "1",
                                "status": "success",
                                "updated": status.updated(),
                                "version_installed": status.version(),
                            }));
                        } else if status.updated() {
                            eprintln!("Updated to v{}", status.version());
                        } else {
                            eprintln!("Already up to date (v{current})");
                        }
                    }
                    Err(e) => {
                        if matches!(*format, OutputFormat::Json) {
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
