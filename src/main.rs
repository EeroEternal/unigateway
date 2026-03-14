mod api_key;
mod app;
mod authz;
mod cli;
mod dto;
mod gateway;
mod mutations;
mod provider;
mod queries;
mod protocol;
mod sdk;
mod service;
mod storage;
mod system;
mod types;

use anyhow::Result;
use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(name = "unigateway", version, about = "Lightweight LLM gateway")]
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
        db: Option<String>,
        #[arg(long, default_value_t = false)]
        no_ui: bool,
    },
    InitAdmin {
        #[arg(long, default_value = "admin")]
        username: String,
        #[arg(long)]
        password: String,
        #[arg(long, default_value = "sqlite://unigateway.db")]
        db: String,
    },
    Metrics {
        #[arg(long, default_value = "sqlite://unigateway.db")]
        db: String,
    },
    CreateService {
        #[arg(long)]
        id: String,
        #[arg(long)]
        name: String,
        #[arg(long, default_value = "sqlite://unigateway.db")]
        db: String,
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
        #[arg(long, default_value = "sqlite://unigateway.db")]
        db: String,
    },
    BindProvider {
        #[arg(long)]
        service_id: String,
        #[arg(long)]
        provider_id: i64,
        #[arg(long, default_value = "sqlite://unigateway.db")]
        db: String,
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
        #[arg(long, default_value = "sqlite://unigateway.db")]
        db: String,
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
        Some(Commands::Serve { bind, db, no_ui }) => {
            let mut config = types::AppConfig::from_env();
            if let Some(bind) = bind {
                config.bind = bind;
            }
            if let Some(db) = db {
                config.db_url = db;
            }
            if no_ui {
                config.enable_ui = false;
            }
            app::run(config).await
        }
        Some(Commands::InitAdmin {
            username,
            password,
            db,
        }) => cli::init_admin(&db, &username, &password).await,
        Some(Commands::Metrics { db }) => cli::print_metrics_snapshot(&db).await,
        Some(Commands::CreateService { id, name, db }) => {
            cli::create_service(&db, &id, &name).await
        }
        Some(Commands::CreateProvider {
            name,
            provider_type,
            endpoint_id,
            base_url,
            api_key,
            model_mapping,
            db,
        }) => {
            let provider_id = cli::create_provider(
                &db,
                &name,
                &provider_type,
                Some(&endpoint_id),
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
            db,
        }) => cli::bind_provider(&db, &service_id, provider_id).await,
        Some(Commands::CreateApiKey {
            key,
            service_id,
            quota_limit,
            qps_limit,
            concurrency_limit,
            db,
        }) => {
            cli::create_api_key(
                &db,
                &key,
                &service_id,
                quota_limit,
                qps_limit,
                concurrency_limit,
            )
            .await
        }
        None => {
            let config = types::AppConfig::from_env();
            app::run(config).await
        }
    }
}
