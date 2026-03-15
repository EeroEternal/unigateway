mod api_key;
mod authz;
mod cli;
mod config;
mod dto;
mod gateway;
mod mcp;
mod middleware;
mod protocol;
mod provider;
mod routing;
mod sdk;
mod server;
mod service;
mod setup;
mod storage;
mod system;
mod types;
mod upgrade;

use anyhow::Result;
use clap::{Parser, Subcommand};

fn config_default() -> String {
    types::default_config_path()
}

#[derive(Parser, Debug)]
#[command(name = "ug", version, about = "UniGateway – lightweight LLM gateway")]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand, Debug)]
enum Commands {
    Serve {
        #[arg(long)]
        bind: Option<String>,
        #[arg(long)]
        config: Option<String>,
        #[arg(long, default_value_t = false)]
        no_ui: bool,
    },
    Metrics {
        #[arg(long, default_value_t = config_default())]
        config: String,
    },
    /// Explore user-facing modes (semantic alias over services).
    Mode {
        #[command(subcommand)]
        action: ModeAction,
    },
    /// Explain how a mode routes requests to providers.
    Route {
        #[command(subcommand)]
        action: RouteAction,
    },
    /// Print tool integration hints for a configured mode.
    Integrations {
        #[arg(long)]
        mode: Option<String>,
        #[arg(long)]
        tool: Option<String>,
        #[arg(long)]
        bind: Option<String>,
        #[arg(long, default_value_t = config_default())]
        config: String,
    },
    /// Run a smoke test against the local gateway for a mode.
    Test {
        #[arg(long)]
        mode: Option<String>,
        #[arg(long)]
        protocol: Option<String>,
        #[arg(long)]
        bind: Option<String>,
        #[arg(long, default_value_t = config_default())]
        config: String,
    },
    /// Inspect current config and local gateway readiness.
    Doctor {
        #[arg(long)]
        mode: Option<String>,
        #[arg(long)]
        bind: Option<String>,
        #[arg(long, default_value_t = config_default())]
        config: String,
    },
    CreateService {
        #[arg(long)]
        id: String,
        #[arg(long)]
        name: String,
        #[arg(long, default_value_t = config_default())]
        config: String,
    },
    CreateProvider {
        #[arg(long)]
        name: String,
        #[arg(long)]
        provider_type: String,
        #[arg(long)]
        endpoint_id: String,
        #[arg(long)]
        base_url: Option<String>,
        #[arg(long)]
        api_key: String,
        #[arg(long)]
        model_mapping: Option<String>,
        #[arg(long, default_value_t = config_default())]
        config: String,
    },
    BindProvider {
        #[arg(long)]
        service_id: String,
        #[arg(long)]
        provider_id: i64,
        #[arg(long, default_value_t = config_default())]
        config: String,
    },
    CreateApiKey {
        #[arg(long)]
        key: String,
        #[arg(long)]
        service_id: String,
        #[arg(long)]
        quota_limit: Option<i64>,
        #[arg(long)]
        qps_limit: Option<f64>,
        #[arg(long)]
        concurrency_limit: Option<i64>,
        #[arg(long, default_value_t = config_default())]
        config: String,
    },
    /// Show, edit, or locate the config file.
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },
    /// Start as an MCP (Model Context Protocol) server over stdio.
    Mcp {
        #[arg(long, default_value_t = config_default())]
        config: String,
    },
    /// Self-upgrade to the latest release.
    Upgrade,
    /// Interactive setup: create service, provider, bind, and API key.
    Quickstart(setup::QuickstartCommand),
}

#[derive(Subcommand, Debug)]
enum ConfigAction {
    /// Print the config file path
    Path,
    /// Print the current config contents
    Show {
        #[arg(long, default_value_t = config_default())]
        config: String,
    },
    /// Open the config file in $EDITOR
    Edit {
        #[arg(long, default_value_t = config_default())]
        config: String,
    },
}

#[derive(Subcommand, Debug)]
enum ModeAction {
    /// List all configured modes.
    List {
        #[arg(long, default_value_t = config_default())]
        config: String,
    },
    /// Show providers and keys for a mode.
    Show {
        mode: String,
        #[arg(long, default_value_t = config_default())]
        config: String,
    },
    /// Set the default mode used by commands that omit --mode.
    Use {
        mode: String,
        #[arg(long, default_value_t = config_default())]
        config: String,
    },
}

#[derive(Subcommand, Debug)]
enum RouteAction {
    /// Explain provider selection for a mode.
    Explain {
        mode: Option<String>,
        #[arg(long, default_value_t = config_default())]
        config: String,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            std::env::var("RUST_LOG")
                .unwrap_or_else(|_| "unigateway=info,tower_http=info".to_string()),
        )
        .init();

    let cli_args = Cli::parse();

    match cli_args.command {
        Some(Commands::Serve {
            bind,
            config: config_path,
            no_ui,
        }) => {
            let mut app_config = types::AppConfig::from_env();
            if let Some(bind) = bind {
                app_config.bind = bind;
            }
            if let Some(c) = config_path {
                app_config.config_path = c;
            }
            if no_ui {
                app_config.enable_ui = false;
            }
            server::run(app_config).await
        }
        Some(Commands::Metrics { config }) => cli::print_metrics_snapshot(&config).await,
        Some(Commands::Mode { action }) => match action {
            ModeAction::List { config } => cli::list_modes(&config).await,
            ModeAction::Show { mode, config } => cli::show_mode(&config, &mode).await,
            ModeAction::Use { mode, config } => cli::use_mode(&config, &mode).await,
        },
        Some(Commands::Route { action }) => match action {
            RouteAction::Explain { mode, config } => {
                cli::explain_route(&config, mode.as_deref()).await
            }
        },
        Some(Commands::Integrations {
            mode,
            tool,
            bind,
            config,
        }) => {
            cli::print_integrations(&config, mode.as_deref(), tool.as_deref(), bind.as_deref())
                .await
        }
        Some(Commands::Test {
            mode,
            protocol,
            bind,
            config,
        }) => {
            cli::test_mode(
                &config,
                mode.as_deref(),
                protocol.as_deref(),
                bind.as_deref(),
            )
            .await
        }
        Some(Commands::Doctor { mode, bind, config }) => {
            cli::doctor(&config, mode.as_deref(), bind.as_deref()).await
        }
        Some(Commands::CreateService { id, name, config }) => {
            cli::create_service(&config, &id, &name).await
        }
        Some(Commands::CreateProvider {
            name,
            provider_type,
            endpoint_id,
            base_url,
            api_key,
            model_mapping,
            config,
        }) => {
            let provider_id = cli::create_provider(
                &config,
                &name,
                &provider_type,
                &endpoint_id,
                base_url.as_deref(),
                &api_key,
                model_mapping.as_deref(),
            )
            .await?;
            println!("provider_id={}", provider_id);
            Ok(())
        }
        Some(Commands::BindProvider {
            service_id,
            provider_id,
            config,
        }) => cli::bind_provider(&config, &service_id, provider_id).await,
        Some(Commands::CreateApiKey {
            key,
            service_id,
            quota_limit,
            qps_limit,
            concurrency_limit,
            config,
        }) => {
            cli::create_api_key(
                &config,
                &key,
                &service_id,
                quota_limit,
                qps_limit,
                concurrency_limit,
            )
            .await
        }
        Some(Commands::Config { action }) => match action {
            ConfigAction::Path => {
                println!("{}", config_default());
                Ok(())
            }
            ConfigAction::Show { config } => {
                let path = std::path::Path::new(&config);
                if path.exists() {
                    let contents = std::fs::read_to_string(path)?;
                    print!("{}", contents);
                } else {
                    println!("Config file not found: {}", config);
                    println!("Run `ug quickstart` to create one.");
                }
                Ok(())
            }
            ConfigAction::Edit { config } => {
                let path = std::path::Path::new(&config);
                if !path.exists() {
                    anyhow::bail!(
                        "Config file not found: {}. Run `ug quickstart` first.",
                        config
                    );
                }
                let editor = std::env::var("EDITOR").unwrap_or_else(|_| "vi".to_string());
                let status = std::process::Command::new(&editor).arg(path).status()?;
                if !status.success() {
                    anyhow::bail!("Editor exited with status: {}", status);
                }
                Ok(())
            }
        },
        Some(Commands::Mcp { config }) => mcp::run(&config).await,
        Some(Commands::Upgrade) => upgrade::run_upgrade().await,
        Some(Commands::Quickstart(command)) => setup::run_quickstart(command).await,
        None => {
            let app_config = types::AppConfig::from_env();
            server::run(app_config).await
        }
    }
}
