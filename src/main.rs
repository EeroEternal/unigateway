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
mod storage;
mod system;
mod types;
mod upgrade;

use anyhow::Result;
use clap::{Parser, Subcommand};
use dialoguer::{Confirm, Input, Password, Select, theme::ColorfulTheme};
use llm_providers::{get_model_for_endpoint, get_providers_data, list_models_for_endpoint};

#[derive(Clone)]
struct RegistryProviderOption {
    display_name: String,
    family_id: String,
    provider_type: String,
    endpoint_id: String,
    default_base_url: String,
    model_ids: Vec<String>,
}

struct ProviderPromptLabels<'a> {
    provider: &'a str,
    model: &'a str,
    base_url: &'a str,
    api_key: &'a str,
}

struct ProviderSetupInput {
    provider_type: Option<String>,
    endpoint_id: Option<String>,
    default_model: Option<String>,
    base_url: Option<String>,
    api_key: Option<String>,
}

struct ProviderSetup {
    provider_type: String,
    endpoint_id: String,
    default_model: Option<String>,
    base_url: Option<String>,
    api_key: String,
}

fn config_default() -> String {
    types::default_config_path()
}

fn registry_provider_type(family_id: &str, base_url: &str) -> Option<&'static str> {
    if family_id == "anthropic" {
        return Some("anthropic");
    }
    if base_url.contains("/v1") {
        return Some("openai");
    }
    None
}

fn preferred_model_for_endpoint(endpoint_id: &str, model_ids: &[String]) -> Option<String> {
    model_ids
        .iter()
        .filter_map(|model_id| {
            get_model_for_endpoint(endpoint_id, model_id).map(|model| {
                (
                    model.supports_tools,
                    model.context_length.unwrap_or(0),
                    model.id.to_string(),
                )
            })
        })
        .max_by(|left, right| left.0.cmp(&right.0).then(left.1.cmp(&right.1)))
        .map(|(_, _, model_id)| model_id)
        .or_else(|| model_ids.first().cloned())
}

fn registry_provider_options() -> Vec<RegistryProviderOption> {
    let mut options = Vec::new();

    for (family_id, provider) in get_providers_data().entries() {
        let Some(endpoint) = provider.endpoints.get("global") else {
            continue;
        };
        if endpoint.region != "global" {
            continue;
        }
        let Some(provider_type) = registry_provider_type(family_id, endpoint.base_url) else {
            continue;
        };

        let endpoint_id = format!("{}:global", family_id);
        let model_ids = list_models_for_endpoint(&endpoint_id).unwrap_or_default();
        options.push(RegistryProviderOption {
            display_name: endpoint.label.to_string(),
            family_id: family_id.to_string(),
            provider_type: provider_type.to_string(),
            endpoint_id,
            default_base_url: endpoint.base_url.to_string(),
            model_ids,
        });
    }

    options.sort_by(|left, right| left.display_name.cmp(&right.display_name));
    options.push(RegistryProviderOption {
        display_name: "Other (OpenAI-compatible)".to_string(),
        family_id: "custom".to_string(),
        provider_type: "openai".to_string(),
        endpoint_id: String::new(),
        default_base_url: String::new(),
        model_ids: Vec::new(),
    });
    options
}

fn resolve_provider_setup(
    labels: ProviderPromptLabels<'_>,
    input: ProviderSetupInput,
) -> ProviderSetup {
    if input.provider_type.is_none() || input.endpoint_id.is_none() || input.api_key.is_none() {
        let theme = ColorfulTheme::default();
        let registry_options = registry_provider_options();
        let display_names: Vec<&str> = registry_options
            .iter()
            .map(|provider| provider.display_name.as_str())
            .collect();
        let selected = input.provider_type.as_deref().and_then(|provider_type| {
            registry_options.iter().position(|provider| {
                provider.family_id == provider_type
                    || provider.provider_type == provider_type
                    || provider.endpoint_id == provider_type
                    || display_names
                        .iter()
                        .any(|name| name.to_lowercase() == provider_type)
            })
        });

        let index = if let Some(selected) = selected {
            selected
        } else if input.provider_type.is_none() {
            Select::with_theme(&theme)
                .with_prompt(labels.provider)
                .items(&display_names)
                .default(0)
                .interact()
                .unwrap()
        } else {
            registry_options.len() - 1
        };

        let provider = &registry_options[index];
        let endpoint_id = input
            .endpoint_id
            .unwrap_or_else(|| provider.endpoint_id.clone());

        let available_models = if endpoint_id.is_empty() {
            provider.model_ids.clone()
        } else {
            list_models_for_endpoint(&endpoint_id).unwrap_or_else(|| provider.model_ids.clone())
        };
        let preferred_model = preferred_model_for_endpoint(&endpoint_id, &available_models)
            .or_else(|| available_models.first().cloned())
            .unwrap_or_default();

        let default_model = input.default_model.or_else(|| {
            if !available_models.is_empty() {
                let default_index = available_models
                    .iter()
                    .position(|model_id| model_id == &preferred_model)
                    .unwrap_or(0);
                Select::with_theme(&theme)
                    .with_prompt(labels.model)
                    .items(&available_models)
                    .default(default_index)
                    .interact()
                    .map(|index| Some(available_models[index].clone()))
                    .unwrap()
            } else if !preferred_model.is_empty() {
                Input::with_theme(&theme)
                    .with_prompt(labels.model)
                    .default(preferred_model)
                    .interact_text()
                    .ok()
            } else {
                Input::with_theme(&theme)
                    .with_prompt(labels.model)
                    .interact_text()
                    .ok()
            }
        });

        let base_url = input.base_url.or_else(|| {
            if !provider.default_base_url.is_empty() {
                let url: String = Input::with_theme(&theme)
                    .with_prompt(labels.base_url)
                    .default(provider.default_base_url.clone())
                    .interact_text()
                    .unwrap();
                if url == provider.default_base_url {
                    None
                } else {
                    Some(url)
                }
            } else {
                let url: String = Input::with_theme(&theme)
                    .with_prompt(labels.base_url)
                    .interact_text()
                    .unwrap();
                Some(url)
            }
        });

        let api_key = input.api_key.unwrap_or_else(|| {
            Password::with_theme(&theme)
                .with_prompt(labels.api_key)
                .interact()
                .unwrap()
        });

        ProviderSetup {
            provider_type: provider.provider_type.clone(),
            endpoint_id,
            default_model,
            base_url,
            api_key,
        }
    } else if let (Some(provider_type), Some(endpoint_id), Some(api_key)) =
        (input.provider_type, input.endpoint_id, input.api_key)
    {
        let default_model = input.default_model.or_else(|| {
            let model_ids = list_models_for_endpoint(&endpoint_id).unwrap_or_default();
            preferred_model_for_endpoint(&endpoint_id, &model_ids)
        });
        ProviderSetup {
            provider_type,
            endpoint_id,
            default_model,
            base_url: input.base_url,
            api_key,
        }
    } else {
        unreachable!("provider setup missing required fields")
    }
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
    Quickstart {
        #[arg(long)]
        service_id: Option<String>,
        #[arg(long)]
        service_name: Option<String>,
        #[arg(long)]
        provider_name: Option<String>,
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
        #[arg(long)]
        backup_provider_name: Option<String>,
        #[arg(long)]
        backup_provider_type: Option<String>,
        #[arg(long)]
        backup_endpoint_id: Option<String>,
        #[arg(long)]
        backup_base_url: Option<String>,
        #[arg(long)]
        backup_api_key: Option<String>,
        #[arg(long)]
        backup_model_mapping: Option<String>,
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
        Some(Commands::Quickstart {
            service_id,
            service_name,
            provider_name,
            provider_type,
            endpoint_id,
            base_url,
            api_key,
            model_mapping,
            backup_provider_name,
            backup_provider_type,
            backup_endpoint_id,
            backup_base_url,
            backup_api_key,
            backup_model_mapping,
            config,
        }) => {
            let interactive = provider_type.is_none() || endpoint_id.is_none() || api_key.is_none();

            if interactive {
                println!("\n  Welcome to UniGateway quickstart!\n");
            }

            let provider_setup = resolve_provider_setup(
                ProviderPromptLabels {
                    provider: "Provider",
                    model: "Default model",
                    base_url: "Base URL",
                    api_key: "API Key",
                },
                ProviderSetupInput {
                    provider_type,
                    endpoint_id,
                    default_model: None,
                    base_url,
                    api_key,
                },
            );

            let backup_requested = backup_provider_name.is_some()
                || backup_provider_type.is_some()
                || backup_endpoint_id.is_some()
                || backup_base_url.is_some()
                || backup_api_key.is_some()
                || backup_model_mapping.is_some();

            let backup_setup = if interactive {
                let add_backup = backup_requested
                    || Confirm::with_theme(&ColorfulTheme::default())
                        .with_prompt("Add a second provider for strong/backup modes?")
                        .default(false)
                        .interact()
                        .unwrap();

                if add_backup {
                    let backup_provider_setup = resolve_provider_setup(
                        ProviderPromptLabels {
                            provider: "Backup provider",
                            model: "Backup default model",
                            base_url: "Backup base URL",
                            api_key: "Backup API Key",
                        },
                        ProviderSetupInput {
                            provider_type: backup_provider_type,
                            endpoint_id: backup_endpoint_id,
                            default_model: None,
                            base_url: backup_base_url,
                            api_key: backup_api_key,
                        },
                    );
                    Some((
                        backup_provider_name
                            .unwrap_or_else(|| {
                                format!("{}-backup", backup_provider_setup.provider_type)
                            })
                            .to_string(),
                        backup_provider_setup.provider_type,
                        backup_provider_setup.endpoint_id,
                        backup_provider_setup.default_model,
                        backup_provider_setup.base_url,
                        backup_provider_setup.api_key,
                    ))
                } else {
                    None
                }
            } else if backup_requested {
                let backup_provider_setup = resolve_provider_setup(
                    ProviderPromptLabels {
                        provider: "Backup provider",
                        model: "Backup default model",
                        base_url: "Backup base URL",
                        api_key: "Backup API Key",
                    },
                    ProviderSetupInput {
                        provider_type: backup_provider_type,
                        endpoint_id: backup_endpoint_id,
                        default_model: None,
                        base_url: backup_base_url,
                        api_key: backup_api_key,
                    },
                );
                Some((
                    backup_provider_name.unwrap_or_else(|| "backup-provider".to_string()),
                    backup_provider_setup.provider_type,
                    backup_provider_setup.endpoint_id,
                    backup_provider_setup.default_model,
                    backup_provider_setup.base_url,
                    backup_provider_setup.api_key,
                ))
            } else {
                None
            };

            let provider_name =
                provider_name.unwrap_or_else(|| provider_setup.provider_type.clone());

            let result = cli::quickstart(
                &config,
                cli::QuickstartParams {
                    service_id: service_id.as_deref(),
                    service_name: service_name.as_deref(),
                    provider_name: &provider_name,
                    provider_type: &provider_setup.provider_type,
                    endpoint_id: &provider_setup.endpoint_id,
                    default_model: provider_setup.default_model.as_deref(),
                    base_url: provider_setup.base_url.as_deref(),
                    api_key: &provider_setup.api_key,
                    model_mapping: model_mapping.as_deref(),
                    backup_provider_name: backup_setup.as_ref().map(|setup| setup.0.as_str()),
                    backup_provider_type: backup_setup.as_ref().map(|setup| setup.1.as_str()),
                    backup_endpoint_id: backup_setup.as_ref().map(|setup| setup.2.as_str()),
                    backup_default_model: backup_setup
                        .as_ref()
                        .and_then(|setup| setup.3.as_deref()),
                    backup_base_url: backup_setup.as_ref().and_then(|setup| setup.4.as_deref()),
                    backup_api_key: backup_setup.as_ref().map(|setup| setup.5.as_str()),
                    backup_model_mapping: backup_model_mapping.as_deref(),
                },
            )
            .await?;

            let created_ids = result
                .modes
                .iter()
                .map(|mode| mode.id.as_str())
                .collect::<Vec<_>>()
                .join(", ");

            println!("\n  Done! Created mode(s): {}.\n", created_ids);
            println!("  Start the gateway:\n");
            println!("    ug serve\n");
            println!("  Inspect the created modes:\n");
            println!("    ug mode list\n");
            if let Some(default_mode) = result.modes.first() {
                println!("  Default mode:\n");
                println!("    {}\n", default_mode.id);
            }

            for mode in &result.modes {
                println!("  Integration hints for mode `{}`:\n", mode.id);
                cli::print_integrations_with_key(
                    &config,
                    Some(&mode.id),
                    None,
                    Some(&mode.key),
                    None,
                )
                .await?;
                println!("\n  Explain routing:\n");
                println!("    ug route explain {}\n", mode.id);
                println!("  Run diagnostics:\n");
                println!("    ug doctor --mode {}\n", mode.id);
                println!("\n  Smoke test after starting the gateway:\n");
                println!("    ug test --mode {}\n", mode.id);
                println!("\n  Tool-specific template examples:\n");
                println!("    ug integrations --mode {} --tool cursor", mode.id);
                println!("    ug integrations --mode {} --tool codex", mode.id);
                println!(
                    "    ug integrations --mode {} --tool claude-code\n",
                    mode.id
                );
            }

            println!("  You can reprint these hints later with:\n");
            println!("    ug integrations --mode <mode>");
            println!(
                "    ug integrations --mode <mode> --tool <cursor|codex|claude-code|env|python|node|curl|anthropic>\n"
            );
            Ok(())
        }
        None => {
            let app_config = types::AppConfig::from_env();
            server::run(app_config).await
        }
    }
}
