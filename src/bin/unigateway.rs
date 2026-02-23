use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            std::env::var("RUST_LOG")
                .unwrap_or_else(|_| "unigateway=info,tower_http=info".to_string()),
        )
        .init();

    let config = unigateway::AppConfig::from_env();
    unigateway::run(config).await
}
