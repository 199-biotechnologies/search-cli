mod classify;
mod cli;
mod config;
mod context;
mod engine;
mod errors;
mod output;
mod providers;
mod types;

use clap::Parser;
use cli::{Cli, Commands, ConfigAction};
use config::{config_check, config_set, config_show, load_config};
use context::AppContext;
use output::OutputFormat;
use std::sync::Arc;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive(tracing::Level::WARN.into()),
        )
        .with_target(false)
        .with_writer(std::io::stderr)
        .init();

    let cli = Cli::parse();
    let format = OutputFormat::detect(cli.json);

    let exit_code = match run(cli, &format).await {
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

async fn run(cli: Cli, format: &OutputFormat) -> Result<i32, errors::SearchError> {
    // Handle bare `search "query"` without subcommand
    let command = if let Some(cmd) = cli.command {
        cmd
    } else if !cli.query_words.is_empty() {
        let query = cli.query_words.join(" ");
        Commands::Search(cli::SearchArgs {
            query,
            mode: types::Mode::Auto,
            count: None,
            providers: None,
        })
    } else {
        // No command and no query — show help
        use clap::CommandFactory;
        Cli::command().print_help().ok();
        println!();
        return Ok(0);
    };

    match command {
        Commands::Search(args) => {
            let config = load_config().map_err(|e| errors::SearchError::Config(e.to_string()))?;
            let count = args.count.unwrap_or(config.settings.count);
            let ctx = Arc::new(AppContext::new(config));

            // Show spinner for human output
            let spinner = if matches!(*format, OutputFormat::Table) && !cli.quiet {
                let sp = indicatif::ProgressBar::new_spinner();
                sp.set_style(
                    indicatif::ProgressStyle::default_spinner()
                        .tick_strings(&[
                            "   ", ".  ", ".. ", "...", " ..", "  .", "   ",
                        ])
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
                engine::run(ctx, &args.query, args.mode, count, &args.providers).await;

            if let Some(sp) = spinner {
                sp.finish_and_clear();
            }

            let response = response?;

            match *format {
                OutputFormat::Json => output::json::render(&response),
                OutputFormat::Table => output::table::render(&response),
            }

            Ok(0)
        }

        Commands::Config { action } => {
            let config = load_config().map_err(|e| errors::SearchError::Config(e.to_string()))?;
            match action {
                ConfigAction::Show => config_show(&config),
                ConfigAction::Set { key, value } => config_set(&key, &value)?,
                ConfigAction::Check => config_check(&config),
            }
            Ok(0)
        }

        Commands::AgentInfo => {
            let config = load_config().map_err(|e| errors::SearchError::Config(e.to_string()))?;
            let ctx = Arc::new(AppContext::new(config));
            let all = providers::build_providers(&ctx);

            let providers_info: Vec<serde_json::Value> = all
                .iter()
                .map(|p| {
                    serde_json::json!({
                        "name": p.name(),
                        "configured": p.is_configured(),
                        "capabilities": p.capabilities(),
                    })
                })
                .collect();

            let info = serde_json::json!({
                "name": "search",
                "version": env!("CARGO_PKG_VERSION"),
                "commands": ["search", "config show", "config set", "config check", "agent-info", "providers", "update"],
                "modes": ["auto", "general", "news", "academic", "people", "deep", "extract", "similar", "scrape", "scholar", "patents", "images", "places"],
                "providers": providers_info,
                "env_prefix": "SEARCH_",
                "config_path": config::config_path().to_string_lossy(),
                "output_formats": ["json", "table"],
                "auto_json_when_piped": true,
            });

            output::json::render_value(&info);
            Ok(0)
        }

        Commands::Providers => {
            let config = load_config().map_err(|e| errors::SearchError::Config(e.to_string()))?;
            let ctx = Arc::new(AppContext::new(config));
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

        Commands::Update { check } => {
            let current = env!("CARGO_PKG_VERSION");
            if check {
                eprintln!("Current version: {current}");
                match self_update::backends::github::Update::configure()
                    .repo_owner("199-biotechnologies")
                    .repo_name("search-cli")
                    .bin_name("search")
                    .current_version(current)
                    .build()
                {
                    Ok(updater) => match updater.get_latest_release() {
                        Ok(release) => {
                            if release.version != current {
                                eprintln!("New version available: {}", release.version);
                                eprintln!("Run `search update` to install");
                            } else {
                                eprintln!("Already up to date");
                            }
                        }
                        Err(e) => eprintln!("Could not check for updates: {e}"),
                    },
                    Err(e) => eprintln!("Update check failed: {e}"),
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
                        if status.updated() {
                            eprintln!("Updated to v{}", status.version());
                        } else {
                            eprintln!("Already up to date (v{current})");
                        }
                    }
                    Err(e) => {
                        eprintln!("Update failed: {e}");
                        eprintln!("You can update manually: cargo install search-cli");
                        return Ok(1);
                    }
                }
            }
            Ok(0)
        }
    }
}
