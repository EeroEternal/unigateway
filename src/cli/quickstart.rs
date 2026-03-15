use anyhow::{Context, Result, bail};
use std::path::Path;

use crate::config::{GatewayState, ProviderModelOptions};

pub struct QuickstartParams<'a> {
    pub service_id: Option<&'a str>,
    pub service_name: Option<&'a str>,
    pub provider_name: &'a str,
    pub provider_type: &'a str,
    pub endpoint_id: &'a str,
    pub default_model: Option<&'a str>,
    pub base_url: Option<&'a str>,
    pub api_key: &'a str,
    pub model_mapping: Option<&'a str>,
    pub backup_provider_name: Option<&'a str>,
    pub backup_provider_type: Option<&'a str>,
    pub backup_endpoint_id: Option<&'a str>,
    pub backup_default_model: Option<&'a str>,
    pub backup_base_url: Option<&'a str>,
    pub backup_api_key: Option<&'a str>,
    pub backup_model_mapping: Option<&'a str>,
}

pub struct QuickstartModeOutput {
    pub id: String,
    pub key: String,
}

pub struct QuickstartResult {
    pub modes: Vec<QuickstartModeOutput>,
}

struct QuickstartModePlan {
    id: String,
    name: String,
    routing_strategy: &'static str,
    bindings: Vec<(i64, i64)>,
}

#[cfg(test)]
pub(crate) fn planned_modes(
    service_id: Option<&str>,
    service_name: Option<&str>,
) -> Vec<(String, String)> {
    if let Some(service_id) = service_id {
        return vec![(
            service_id.to_string(),
            service_name.unwrap_or(service_id).to_string(),
        )];
    }

    vec![
        ("fast".to_string(), "Fast".to_string()),
        ("strong".to_string(), "Strong".to_string()),
        ("backup".to_string(), "Backup".to_string()),
    ]
}

fn quickstart_mode_plans(
    service_id: Option<&str>,
    service_name: Option<&str>,
    primary_provider_id: i64,
    secondary_provider_id: Option<i64>,
) -> Vec<QuickstartModePlan> {
    if let Some(service_id) = service_id {
        let mut bindings = vec![(primary_provider_id, 0)];
        let routing_strategy = if let Some(secondary_provider_id) = secondary_provider_id {
            bindings.push((secondary_provider_id, 1));
            "fallback"
        } else {
            "round_robin"
        };

        return vec![QuickstartModePlan {
            id: service_id.to_string(),
            name: service_name.unwrap_or(service_id).to_string(),
            routing_strategy,
            bindings,
        }];
    }

    let strong_bindings = secondary_provider_id
        .map(|provider_id| vec![(provider_id, 0)])
        .unwrap_or_else(|| vec![(primary_provider_id, 0)]);

    let mut backup_bindings = vec![(primary_provider_id, 0)];
    if let Some(secondary_provider_id) = secondary_provider_id {
        backup_bindings.push((secondary_provider_id, 1));
    }

    vec![
        QuickstartModePlan {
            id: "fast".to_string(),
            name: "Fast".to_string(),
            routing_strategy: "round_robin",
            bindings: vec![(primary_provider_id, 0)],
        },
        QuickstartModePlan {
            id: "strong".to_string(),
            name: "Strong".to_string(),
            routing_strategy: "round_robin",
            bindings: strong_bindings,
        },
        QuickstartModePlan {
            id: "backup".to_string(),
            name: "Backup".to_string(),
            routing_strategy: "fallback",
            bindings: backup_bindings,
        },
    ]
}

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
        .create_provider(
            name,
            provider_type,
            endpoint_id,
            base_url,
            api_key,
            model_mapping,
        )
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
    params: QuickstartParams<'_>,
) -> Result<QuickstartResult> {
    let state = GatewayState::load(Path::new(config_path)).await?;
    let primary_provider_id = state
        .create_provider_with_models(
            params.provider_name,
            params.provider_type,
            params.endpoint_id,
            params.base_url,
            params.api_key,
            ProviderModelOptions {
                default_model: params.default_model,
                model_mapping: params.model_mapping,
            },
        )
        .await;

    let secondary_provider_id = match (
        params.backup_provider_name,
        params.backup_provider_type,
        params.backup_endpoint_id,
        params.backup_api_key,
    ) {
        (Some(name), Some(provider_type), Some(endpoint_id), Some(api_key)) => Some(
            state
                .create_provider_with_models(
                    name,
                    provider_type,
                    endpoint_id,
                    params.backup_base_url,
                    api_key,
                    ProviderModelOptions {
                        default_model: params.backup_default_model,
                        model_mapping: params.backup_model_mapping,
                    },
                )
                .await,
        ),
        (None, None, None, None) => None,
        _ => bail!("backup provider requires name, provider_type, endpoint_id, and api_key"),
    };

    let planned = quickstart_mode_plans(
        params.service_id,
        params.service_name,
        primary_provider_id,
        secondary_provider_id,
    );
    let default_mode = planned.first().map(|mode| mode.id.clone());
    let mut modes = Vec::new();
    for plan in planned {
        let key = format!("ugk_{}", hex::encode(rand::random::<[u8; 16]>()));
        state.create_service(&plan.id, &plan.name).await;
        state
            .set_service_routing_strategy(&plan.id, plan.routing_strategy)
            .await?;
        for (provider_id, priority) in &plan.bindings {
            state
                .bind_provider_to_service_with_priority(&plan.id, *provider_id, *priority)
                .await?;
        }
        state.create_api_key(&key, &plan.id, None, None, None).await;
        modes.push(QuickstartModeOutput { id: plan.id, key });
    }

    if let Some(default_mode) = default_mode {
        state.set_default_mode(&default_mode).await?;
    }

    state.persist_if_dirty().await?;
    Ok(QuickstartResult { modes })
}
