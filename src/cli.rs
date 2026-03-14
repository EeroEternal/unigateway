use anyhow::{Context, Result};
use std::path::Path;

use crate::config::GatewayState;

pub async fn create_service(config_path: &str, service_id: &str, name: &str) -> Result<()> {
    let state = GatewayState::load(Path::new(config_path)).await?;
    state.create_service(service_id, name).await;
    state.persist_if_dirty().await
}

pub async fn create_provider(
    config_path: &str,
    name: &str,
    provider_type: &str,
    endpoint_id: &str,
    base_url: Option<&str>,
    api_key: &str,
    model_mapping: Option<&str>,
) -> Result<i64> {
    let state = GatewayState::load(Path::new(config_path)).await?;
    let id = state
        .create_provider(name, provider_type, endpoint_id, base_url, api_key, model_mapping)
        .await;
    state.persist_if_dirty().await?;
    Ok(id)
}

pub async fn bind_provider(config_path: &str, service_id: &str, provider_id: i64) -> Result<()> {
    let state = GatewayState::load(Path::new(config_path)).await?;
    state
        .bind_provider_to_service(service_id, provider_id)
        .await
        .with_context(|| format!("bind provider_id {} to service {}", provider_id, service_id))?;
    state.persist_if_dirty().await
}

pub async fn create_api_key(
    config_path: &str,
    key: &str,
    service_id: &str,
    quota_limit: Option<i64>,
    qps_limit: Option<f64>,
    concurrency_limit: Option<i64>,
) -> Result<()> {
    let state = GatewayState::load(Path::new(config_path)).await?;
    state
        .create_api_key(key, service_id, quota_limit, qps_limit, concurrency_limit)
        .await;
    state.persist_if_dirty().await
}

pub async fn quickstart(
    config_path: &str,
    service_id: &str,
    service_name: &str,
    provider_name: &str,
    provider_type: &str,
    endpoint_id: &str,
    base_url: Option<&str>,
    api_key: &str,
    model_mapping: Option<&str>,
) -> Result<String> {
    let key = format!("ugk_{}", hex::encode(rand::random::<[u8; 16]>()));
    let state = GatewayState::load(Path::new(config_path)).await?;
    state.create_service(service_id, service_name).await;
    let provider_id = state
        .create_provider(provider_name, provider_type, endpoint_id, base_url, api_key, model_mapping)
        .await;
    state.bind_provider_to_service(service_id, provider_id).await?;
    state.create_api_key(&key, service_id, None, None, None).await;
    state.persist_if_dirty().await?;
    Ok(key)
}

pub async fn print_metrics_snapshot(config_path: &str) -> Result<()> {
    let state = GatewayState::load(Path::new(config_path)).await?;
    let (total, openai_total, anthropic_total) = state.metrics_snapshot().await;
    println!("unigateway_requests_total {}", total);
    println!(
        "unigateway_requests_by_endpoint_total{{endpoint=\"/v1/chat/completions\"}} {}",
        openai_total
    );
    println!(
        "unigateway_requests_by_endpoint_total{{endpoint=\"/v1/messages\"}} {}",
        anthropic_total
    );
    Ok(())
}
