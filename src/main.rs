mod api_key;
mod server;
mod authz;
mod cli;
mod config;
mod dto;
mod gateway;
mod middleware;
mod provider;
mod protocol;
mod routing;
mod sdk;
mod service;
mod storage;
mod system;
mod types;
mod upgrade;

use anyhow::Result;
use clap::{Parser, Subcommand};
use dialoguer::{Input, Password, Select, theme::ColorfulTheme};

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
        Some(Commands::Serve { bind, config: config_path, no_ui }) => {
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
        Some(Commands::Config { action }) => {
            match action {
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
                        anyhow::bail!("Config file not found: {}. Run `ug quickstart` first.", config);
                    }
                    let editor = std::env::var("EDITOR").unwrap_or_else(|_| "vi".to_string());
                    let status = std::process::Command::new(&editor).arg(path).status()?;
                    if !status.success() {
                        anyhow::bail!("Editor exited with status: {}", status);
                    }
                    Ok(())
                }
            }
        }
        Some(Commands::Upgrade) => {
            upgrade::run_upgrade().await
        }
        Some(Commands::Quickstart {
            service_id,
            service_name,
            provider_name,
            provider_type,
            endpoint_id,
            base_url,
            api_key,
            model_mapping,
            config,
        }) => {
            let interactive = provider_type.is_none() || endpoint_id.is_none() || api_key.is_none();

            // Known providers: (display_name, provider_type, default_base_url, default_model)
            let known_providers: &[(&str, &str, &str, &str)] = &[
                ("OpenAI",      "openai",    "https://api.openai.com",       "gpt-4o"),
                ("Anthropic",   "anthropic", "https://api.anthropic.com",    "claude-sonnet-4-20250514"),
                ("DeepSeek",    "openai",    "https://api.deepseek.com",     "deepseek-chat"),
                ("Groq",        "openai",    "https://api.groq.com/openai",  "llama-3.3-70b-versatile"),
                ("MiniMax",     "openai",    "https://api.minimax.chat/v1",  "MiniMax-Text-01"),
                ("Ollama",      "openai",    "http://localhost:11434",       "llama3"),
                ("OpenRouter",  "openai",    "https://openrouter.ai/api",    "openai/gpt-4o"),
                ("Together AI", "openai",    "https://api.together.xyz",     "meta-llama/Llama-3-70b-chat-hf"),
                ("Other (OpenAI-compatible)", "openai", "", ""),
            ];

            let (provider_type, endpoint_id, base_url, api_key) = if interactive {
                let theme = ColorfulTheme::default();
                println!("\n  Welcome to UniGateway quickstart!\n");

                let display_names: Vec<&str> = known_providers.iter().map(|(name, _, _, _)| *name).collect();
                let selected = provider_type.as_deref().and_then(|pt| {
                    known_providers.iter().position(|(_, _, url, _)| url.contains(pt) || display_names.iter().position(|n| n.to_lowercase() == pt).is_some())
                });

                let idx = if selected.is_some() {
                    selected.unwrap()
                } else if provider_type.is_none() {
                    Select::with_theme(&theme)
                        .with_prompt("Provider")
                        .items(&display_names)
                        .default(0)
                        .interact()
                        .unwrap()
                } else {
                    known_providers.len() - 1
                };

                let (_, pt, default_url, default_model) = known_providers[idx];

                let eid = endpoint_id.unwrap_or_else(|| {
                    if !default_model.is_empty() {
                        Input::with_theme(&theme)
                            .with_prompt("Model")
                            .default(default_model.to_string())
                            .interact_text()
                            .unwrap()
                    } else {
                        Input::with_theme(&theme)
                            .with_prompt("Model")
                            .interact_text()
                            .unwrap()
                    }
                });

                let bu = base_url.or_else(|| {
                    if !default_url.is_empty() {
                        let url: String = Input::with_theme(&theme)
                            .with_prompt("Base URL")
                            .default(default_url.to_string())
                            .interact_text()
                            .unwrap();
                        if url == default_url { None } else { Some(url) }
                    } else {
                        let url: String = Input::with_theme(&theme)
                            .with_prompt("Base URL")
                            .interact_text()
                            .unwrap();
                        Some(url)
                    }
                });

                let ak = api_key.unwrap_or_else(|| {
                    Password::with_theme(&theme)
                        .with_prompt("API Key")
                        .interact()
                        .unwrap()
                });

                (pt.to_string(), eid, bu, ak)
            } else {
                (provider_type.unwrap(), endpoint_id.unwrap(), base_url, api_key.unwrap())
            };

            let service_id = service_id.unwrap_or_else(|| "default".to_string());
            let service_name = service_name.unwrap_or_else(|| "Default".to_string());
            let provider_name = provider_name.unwrap_or_else(|| provider_type.clone());

            let key = cli::quickstart(
                &config,
                &service_id,
                &service_name,
                &provider_name,
                &provider_type,
                &endpoint_id,
                base_url.as_deref(),
                &api_key,
                model_mapping.as_deref(),
            )
            .await?;

            println!("\n  Done! Your gateway API key:\n");
            println!("    {}\n", key);
            println!("  Start the gateway:\n");
            println!("    ug serve\n");
            Ok(())
        }
        None => {
            let app_config = types::AppConfig::from_env();
            server::run(app_config).await
        }
    }
}
