mod admin;
mod authz;
mod config;
mod dto;
mod gateway;
mod host_adapter;
mod middleware;
mod routing;
mod sdk;
mod server;
mod system;
mod telemetry;
mod types;
mod upgrade;

use anyhow::Result;
use clap::{CommandFactory, Parser};
use clap_complete::generate;
use std::io;
use unigateway_cli as cli;
use unigateway_cli::{
    Cli, Commands, ConfigAction, KeyAction, ModeAction, ProviderAction, RouteAction, ServiceAction,
    run_guide,
};

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
        }) => cli::interactive_launch(&config, tool, mode, bind).await,
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
                println!("{}", types::default_config_path());
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
                    anyhow::bail!("Config file not found: {}. Run `ug guide` first.", config);
                }
                let editor = std::env::var("EDITOR").unwrap_or_else(|_| "vi".to_string());
                let status = std::process::Command::new(&editor).arg(path).status()?;
                if !status.success() {
                    anyhow::bail!("Editor exited with status: {}", status);
                }
                Ok(())
            }
            ConfigAction::Get { key, config } => cli::config_get(&config, &key).await,
            ConfigAction::Set { key, value, config } => {
                cli::config_set(&config, &key, &value).await
            }
        },
        Some(Commands::Mcp { config }) => admin::run_mcp(&config).await,
        Some(Commands::Upgrade) => upgrade::run_upgrade().await,
        Some(Commands::Guide(command)) => run_guide(*command).await,
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
