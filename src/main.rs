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
    /// One-shot init: create service, provider, bind, and API key; print the key.
    Quickstart {
        #[arg(long, default_value = "default")]
        service_id: String,
        #[arg(long, default_value = "Default")]
        service_name: String,
        #[arg(long, default_value = "default")]
        provider_name: String,
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
            println!("{}", key);
            Ok(())
        }
        None => {
            let app_config = types::AppConfig::from_env();
            server::run(app_config).await
        }
    }
}
