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
use clap::{CommandFactory, Parser, Subcommand};
use clap_complete::{generate, Shell};
use std::io;

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
    /// Start the gateway server.
    #[command(about = "Start the gateway server", long_about = "Starts the UniGateway server.

Examples:
  # Start server in background (default)
  ug serve

  # Start in foreground (blocking)
  ug serve --foreground

  # Bind to a specific address
  ug serve --bind 0.0.0.0:8000")]
    Serve {
        #[arg(long)]
        bind: Option<String>,
        #[arg(long)]
        config: Option<String>,
        #[arg(long, default_value_t = false)]
        no_ui: bool,
        /// Run in the foreground (blocking).
        #[arg(short, long, default_value_t = false)]
        foreground: bool,
        /// Internal flag for detached process.
        #[arg(long, hide = true)]
        detached: bool,
    },
    /// Stop the background gateway process.
    #[command(about = "Stop the background gateway process")]
    Stop,
    /// Check the status of the background gateway process.
    #[command(about = "Check the status of the background gateway process")]
    Status,
    /// View the background gateway logs.
    #[command(about = "View the background gateway logs", long_about = "View the background gateway logs.

Examples:
  # Print current logs
  ug logs

  # Follow log output (tail -f)
  ug logs --follow")]
    Logs {
        /// Tail the logs.
        #[arg(short, long, default_value_t = false)]
        follow: bool,
    },
    /// Print a snapshot of current metrics.
    #[command(about = "Print a snapshot of current metrics")]
    Metrics {
        #[arg(long, default_value_t = config_default())]
        config: String,
    },
    /// Explore user-facing modes (semantic alias over services).
    #[command(alias = "models", about = "Explore user-facing modes", long_about = "Explore user-facing modes (semantic alias over services).

Examples:
  # List all modes
  ug mode list

  # Show details for a specific mode
  ug mode show default

  # Set the default mode
  ug mode use fast")]
    Mode {
        #[command(subcommand)]
        action: ModeAction,
    },
    /// Explain how a mode routes requests to providers.
    #[command(about = "Explain how a mode routes requests to providers", long_about = "Explain how a mode routes requests to providers.

Examples:
  # Explain routing for the default mode
  ug route explain

  # Explain routing for a specific mode
  ug route explain --mode fast")]
    Route {
        #[command(subcommand)]
        action: RouteAction,
    },
    /// Print tool integration hints for a configured mode.
    #[command(about = "Print tool integration hints for a configured mode", long_about = "Print tool integration hints for a configured mode.

Examples:
  # Show integration hints for all tools
  ug integrations

  # Show hints for Cursor
  ug integrations --tool cursor

  # Show hints for a specific mode
  ug integrations --mode fast")]
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
    /// Interactive launch/setup for AI tools.
    #[command(about = "Interactive launch/setup for AI tools", long_about = "Interactive launch/setup for AI tools.

Examples:
  # Launch the interactive tool picker
  ug launch

  # Directly show setup for a tool
  ug launch claudecode")]
    Launch {
        /// Optional tool name to bypass picker
        tool: Option<String>,
        #[arg(long)]
        mode: Option<String>,
        #[arg(long)]
        bind: Option<String>,
        #[arg(long, default_value_t = config_default())]
        config: String,
    },
    /// Run a smoke test against the local gateway for a mode.
    #[command(about = "Run a smoke test against the local gateway for a mode", long_about = "Run a smoke test against the local gateway for a mode.

Examples:
  # Test the default mode
  ug test

  # Test a specific mode
  ug test --mode fast

  # Test using Anthropic protocol
  ug test --protocol anthropic")]
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
    #[command(about = "Inspect current config and local gateway readiness")]
    Doctor {
        #[arg(long)]
        mode: Option<String>,
        #[arg(long)]
        bind: Option<String>,
        #[arg(long, default_value_t = config_default())]
        config: String,
    },
    /// Manage services (logical groupings of models).
    #[command(about = "Manage services (logical groupings of models)", long_about = "Manage services (logical groupings of models).

Examples:
  # List services
  ug service list

  # Create a new service
  ug service create --id my-service --name \"My Service\"")]
    Service {
        #[command(subcommand)]
        action: ServiceAction,
    },
    /// Manage LLM providers.
    #[command(about = "Manage LLM providers", long_about = "Manage LLM providers.
    
Examples:
  # List all providers
  ug provider list

  # Create a new OpenAI provider
  ug provider create --name deepseek --provider-type openai --base-url https://api.deepseek.com --api-key sk-xxx --endpoint-id deepseek-chat

  # Bind a provider to a service
  ug provider bind --service-id default --provider-id 1")]
    Provider {
        #[command(subcommand)]
        action: ProviderAction,
    },
    /// Manage API keys.
    #[command(about = "Manage API keys", long_about = "Manage API keys.

Examples:
  # Create a new API key
  ug key create --key my-key --service-id default

  # Create a key with quota limit
  ug key create --key limited-key --service-id default --quota-limit 1000")]
    Key {
        #[command(subcommand)]
        action: KeyAction,
    },
    /// Show, edit, or locate the config file.
    #[command(about = "Show, edit, or locate the config file")]
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },
    /// Start as an MCP (Model Context Protocol) server over stdio.
    #[command(about = "Start as an MCP (Model Context Protocol) server over stdio")]
    Mcp {
        #[arg(long, default_value_t = config_default())]
        config: String,
    },
    /// Self-upgrade to the latest release.
    #[command(about = "Self-upgrade to the latest release")]
    Upgrade,
    /// Interactive setup guide: create service, provider, bind, and API key.
    #[command(alias = "quickstart", about = "Interactive setup guide")]
    Guide(Box<setup::GuideCommand>),
    /// Generate shell completion scripts.
    #[command(about = "Generate shell completion scripts", long_about = "Generate shell completion scripts.

Examples:
  # Generate Zsh completion
  ug completion zsh > _ug

  # Generate Bash completion
  ug completion bash > /etc/bash_completion.d/ug")]
    Completion {
        #[arg(value_enum)]
        shell: Shell,
    },
}

#[derive(Subcommand, Debug)]
enum ServiceAction {
    /// List all configured services.
    List {
        #[arg(long, default_value_t = config_default())]
        config: String,
        #[arg(long)]
        json: bool,
    },
    /// Create a new service.
    Create {
        #[arg(long)]
        id: Option<String>,
        #[arg(long)]
        name: Option<String>,
        #[arg(long, default_value_t = config_default())]
        config: String,
    },
}

#[derive(Subcommand, Debug)]
enum ProviderAction {
    /// List all registered providers.
    List {
        #[arg(long, default_value_t = config_default())]
        config: String,
        #[arg(long)]
        json: bool,
    },
    /// Register a new provider.
    Create {
        #[arg(long)]
        name: Option<String>,
        #[arg(long)]
        provider_type: Option<String>,
        #[arg(long)]
        endpoint_id: Option<String>,
        #[arg(long)]
        base_url: Option<String>,
        #[arg(long)]
        api_key: Option<String>,
        #[arg(long)]
        model_mapping: Option<String>,
        #[arg(long, default_value_t = config_default())]
        config: String,
    },
    /// Bind a provider to a service.
    Bind {
        #[arg(long)]
        service_id: String,
        #[arg(long)]
        provider_id: i64,
        #[arg(long, default_value_t = config_default())]
        config: String,
    },
}

#[derive(Subcommand, Debug)]
enum KeyAction {
    /// Create a new API key.
    Create {
        #[arg(long)]
        key: Option<String>,
        #[arg(long)]
        service_id: Option<String>,
        #[arg(long)]
        quota_limit: Option<i64>,
        #[arg(long)]
        qps_limit: Option<f64>,
        #[arg(long)]
        concurrency_limit: Option<i64>,
        #[arg(long, default_value_t = config_default())]
        config: String,
    },
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
    /// Get a config value.
    Get {
        key: String,
        #[arg(long, default_value_t = config_default())]
        config: String,
    },
    /// Set a config value.
    Set {
        key: String,
        value: String,
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
        #[arg(long)]
        json: bool,
    },
    /// Show providers and keys for a mode.
    Show {
        mode: String,
        #[arg(long, default_value_t = config_default())]
        config: String,
        #[arg(long)]
        json: bool,
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
            foreground,
            detached,
        }) => {
            if !foreground && !detached {
                cli::daemonize()?;
                return Ok(());
            }

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
        Some(Commands::Stop) => cli::stop_server(),
        Some(Commands::Status) => cli::status_server(),
        Some(Commands::Logs { follow }) => cli::view_logs(follow),
        Some(Commands::Metrics { config }) => cli::print_metrics_snapshot(&config).await,
        Some(Commands::Mode { action }) => match action {
            ModeAction::List { config, json } => cli::list_modes(&config, json).await,
            ModeAction::Show { mode, config, json } => cli::show_mode(&config, &mode, json).await,
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
        Some(Commands::Launch {
            tool,
            mode,
            bind,
            config,
        }) => {
            cli::interactive_launch(&config, tool, mode, bind).await
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
        Some(Commands::Service { action }) => match action {
            ServiceAction::List { config, json } => cli::list_services(&config, json).await,
            ServiceAction::Create { id, name, config } => match (id, name) {
                (Some(id), Some(name)) => cli::create_service(&config, &id, &name).await,
                _ => cli::interactive_create_service(&config).await,
            },
        },
        Some(Commands::Provider { action }) => match action {
            ProviderAction::Create {
                name,
                provider_type,
                endpoint_id,
                base_url,
                api_key,
                model_mapping,
                config,
            } => match (name, provider_type, endpoint_id, api_key) {
                (Some(name), Some(provider_type), Some(endpoint_id), Some(api_key)) => {
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
                _ => cli::interactive_create_provider(&config).await,
            },
            ProviderAction::Bind {
                service_id,
                provider_id,
                config,
            } => cli::bind_provider(&config, &service_id, provider_id).await,
            ProviderAction::List { config, json } => cli::list_providers(&config, json).await,
        },
        Some(Commands::Key { action }) => match action {
            KeyAction::Create {
                key,
                service_id,
                quota_limit,
                qps_limit,
                concurrency_limit,
                config,
            } => match (key, service_id) {
                (Some(key), Some(service_id)) => {
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
                _ => cli::interactive_create_api_key(&config).await,
            },
        },
        Some(Commands::Completion { shell }) => {
            generate(shell, &mut Cli::command(), "ug", &mut io::stdout());
            Ok(())
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
                    println!("Run `ug guide` to create one.");
                }
                Ok(())
            }
            ConfigAction::Edit { config } => {
                let path = std::path::Path::new(&config);
                if !path.exists() {
                    anyhow::bail!(
                        "Config file not found: {}. Run `ug guide` first.",
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
            ConfigAction::Get { key, config } => cli::config_get(&config, &key).await,
            ConfigAction::Set { key, value, config } => cli::config_set(&config, &key, &value).await,
        },
        Some(Commands::Mcp { config }) => mcp::run(&config).await,
        Some(Commands::Upgrade) => upgrade::run_upgrade().await,
        Some(Commands::Guide(command)) => setup::run_guide(*command).await,
        None => {
            if cli::is_running().is_none() {
                cli::daemonize()?;
            } else {
                println!("UniGateway is already running.");
                println!("Use 'ug stop' to stop it, or 'ug status' to check status.");
            }
            Ok(())
        }
    }
}
