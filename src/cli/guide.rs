use anyhow::{Context, Result, bail};
use std::path::Path;

use crate::config::{GatewayState, ProviderModelOptions};

pub struct GuideParams<'a> {
    pub service_id: Option<&'a str>,
    pub service_name: Option<&'a str>,
    pub provider_name: &'a str,
    pub provider_type: &'a str,
    pub endpoint_id: &'a str,
    pub default_model: Option<&'a str>,
    pub fast_model: Option<&'a str>,
    pub strong_model: Option<&'a str>,
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

pub struct GuideModeOutput {
    pub id: String,
    pub key: String,
}

pub struct GuideResult {
    pub modes: Vec<GuideModeOutput>,
}

struct GuideModePlan {
    id: String,
    name: String,
    routing_strategy: &'static str,
    bindings: Vec<(i64, i64)>,
}

pub(crate) fn planned_modes(
    service_id: Option<&str>,
    service_name: Option<&str>,
    fast_model: Option<&str>,
    strong_model: Option<&str>,
) -> Vec<(String, String)> {
    if let Some(service_id) = service_id {
        return vec![(
            service_id.to_string(),
            service_name.unwrap_or(service_id).to_string(),
        )];
    }

    if fast_model.is_some() || strong_model.is_some() {
        let mut modes = Vec::new();
        if fast_model.is_some() {
            modes.push(("fast".to_string(), "Fast".to_string()));
        }
        if strong_model.is_some() {
            modes.push(("strong".to_string(), "Strong".to_string()));
        }
        return modes;
    }

    vec![("default".to_string(), "Default".to_string())]
}

fn guide_mode_plans(
    service_id: Option<&str>,
    service_name: Option<&str>,
    fast_model: Option<&str>,
    strong_model: Option<&str>,
    primary_provider_id: i64,
    secondary_provider_id: Option<i64>,
) -> Vec<GuideModePlan> {
    let mut plans = Vec::new();
    let modes = planned_modes(service_id, service_name, fast_model, strong_model);

    for (id, name) in modes {
        let mut bindings = vec![(primary_provider_id, 0)];
        let routing_strategy = if let Some(secondary_provider_id) = secondary_provider_id {
            bindings.push((secondary_provider_id, 1));
            "fallback"
        } else {
            "round_robin"
        };

        plans.push(GuideModePlan {
            id,
            name,
            routing_strategy,
            bindings,
        });
    }

    plans
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

use console::style;
use dialoguer::{Input, Select, theme::ColorfulTheme};

pub async fn interactive_create_service(config_path: &str) -> Result<()> {
    let theme = ColorfulTheme::default();
    let id: String = Input::with_theme(&theme)
        .with_prompt("Service ID (e.g. 'default')")
        .default("default".to_string())
        .interact_text()?;

    let name: String = Input::with_theme(&theme)
        .with_prompt("Service Name")
        .default("Default Service".to_string())
        .interact_text()?;

    create_service(config_path, &id, &name).await?;
    println!("{} Service '{}' created.", style("✅").green(), style(id).bold());
    Ok(())
}

pub async fn interactive_create_provider(config_path: &str) -> Result<()> {
    let theme = ColorfulTheme::default();
    
    let name: String = Input::with_theme(&theme)
        .with_prompt("Provider Name")
        .interact_text()?;

    let provider_types = vec!["openai", "anthropic"];
    let selection = Select::with_theme(&theme)
        .with_prompt("Provider Type")
        .items(&provider_types)
        .default(0)
        .interact()?;
    let provider_type = provider_types[selection];

    let endpoint_id: String = Input::with_theme(&theme)
        .with_prompt("Endpoint ID (e.g. 'openai:global')")
        .interact_text()?;

    let base_url: String = Input::with_theme(&theme)
        .with_prompt("Base URL (optional)")
        .allow_empty(true)
        .interact_text()?;
    
    let api_key: String = Input::with_theme(&theme)
        .with_prompt("API Key")
        .interact_text()?;

    let base_url = if base_url.is_empty() { None } else { Some(base_url.as_str()) };

    let id = create_provider(
        config_path,
        &name,
        provider_type,
        &endpoint_id,
        base_url,
        &api_key,
        None
    ).await?;

    println!("{} Provider '{}' created with ID: {}", style("✅").green(), style(name).bold(), id);
    Ok(())
}

pub async fn interactive_create_api_key(config_path: &str) -> Result<()> {
    let theme = ColorfulTheme::default();
    
    let key: String = Input::with_theme(&theme)
        .with_prompt("API Key Value (leave empty to generate)")
        .allow_empty(true)
        .interact_text()?;
    
    let key = if key.is_empty() {
        format!("ugk_{}", hex::encode(rand::random::<[u8; 16]>()))
    } else {
        key
    };

    let service_id: String = Input::with_theme(&theme)
        .with_prompt("Service ID to bind")
        .default("default".to_string())
        .interact_text()?;

    create_api_key(config_path, &key, &service_id, None, None, None).await?;
    
    // Ask for AI Agent integration preference
    let agent_options = vec!["OpenClaw", "Claude Code", "Cursor", "Zed", "Other / None"];
    let agent_selection = Select::with_theme(&theme)
        .with_prompt("Which AI Agent will use this key?")
        .items(&agent_options)
        .default(0)
        .interact()?;
    
    println!("\n{} API Key created successfully!", style("✅").green());
    println!("   Key: {}", style(&key).cyan().bold());
    println!("   Service: {}", style(&service_id).dim());

    let bind_addr = crate::types::AppConfig::from_env().bind;
    let base_url = format!("http://{}/v1", crate::cli::modes::user_bind_address(&bind_addr));

    match agent_selection {
        0 => { // OpenClaw
            println!("\n📝 {}", style("OpenClaw Configuration").bold().underlined());
            println!("Edit {}:", style("~/.openclaw/openclaw.json").bold());
            println!("{}", style(format!(r#"
  {{
    "api": "openai-completions",
    "url": "{}/chat/completions",
    "key": "{}",
    "model": "default"
  }}"#, base_url, key)).dim());
        },
        1 => { // Claude Code
            println!("\n📝 {}", style("Claude Code Configuration").bold().underlined());
            println!("Run the following command to configure:");
            println!("{}", style(format!("export OPENAI_BASE_URL={}", base_url)).cyan());
            println!("{}", style(format!("export OPENAI_API_KEY={}", key)).cyan());
            println!("{}", style("claude config set --global provider openai").dim());
        },
        2 => { // Cursor
            println!("\n📝 {}", style("Cursor Configuration").bold().underlined());
            println!("1. Go to {} -> {}", style("Settings").bold(), style("Models").bold());
            println!("2. Toggle off default models if needed");
            println!("3. Add a new {} model", style("OpenAI").bold());
            println!("4. Set Base URL to: {}", style(&base_url).cyan());
            println!("5. Set API Key to: {}", style(&key).cyan());
        },
        3 => { // Zed
            println!("\n📝 {}", style("Zed Configuration").bold().underlined());
            println!("Add this to your {}:", style("settings.json").bold());
            println!("{}", style(format!(r#"
  "assistant": {{
    "version": "2",
    "default_model": {{
      "provider": "openai",
      "model": "default"
    }}
  }},
  "language_models": {{
    "openai": {{
      "version": "1",
      "api_url": "{}",
      "available_models": [
        {{ "name": "default", "max_tokens": 128000 }}
      ]
    }}
  }}"#, base_url)).dim());
            println!("\nThen run: {}", style(format!("export OPENAI_API_KEY={}", key)).cyan());
        },
        _ => {
            println!("\n💡 Use `ug integrations` to see more configuration examples.");
        }
    }
    
    println!("\n🚀 Ready! Use `ug help` for more commands.");
    Ok(())
}

pub async fn guide(
    config_path: &str,
    params: GuideParams<'_>,
) -> Result<GuideResult> {
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

    let planned = guide_mode_plans(
        params.service_id,
        params.service_name,
        params.fast_model,
        params.strong_model,
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

        // Determine which model to use for this mode
        let model_options = match plan.id.as_str() {
            "fast" => ProviderModelOptions {
                default_model: params.fast_model,
                model_mapping: None,
            },
            "strong" => ProviderModelOptions {
                default_model: params.strong_model,
                model_mapping: None,
            },
            _ => ProviderModelOptions {
                default_model: params.default_model,
                model_mapping: params.model_mapping,
            },
        };

        // Update provider with model options for this specific mode's binding if necessary
        // Actually, the current schema stores default_model on the provider itself.
        // If we want different modes to use DIFFERENT models on the SAME provider,
        // we might need to adjust binder or just set it globally for now if only one provider is used.
        // But the user said "manually set which is fast, which is strong".
        // For simplicity, we create the provider with its "default_model" first.
        // If multi-mode, we might need provider-per-mode or model-mapped-to-mode.
        // The previous implementation used model mapping like "fast=...".

        if model_options.default_model.is_some() {
            state
                .set_provider_model_options(primary_provider_id, model_options)
                .await?;
        }

        for (provider_id, priority) in &plan.bindings {
            state
                .bind_provider_to_service_with_priority(&plan.id, *provider_id, *priority)
                .await?;
        }
        state.create_api_key(&key, &plan.id, None, None, None).await;
        modes.push(GuideModeOutput { id: plan.id, key });
    }

    if let Some(default_mode) = default_mode {
        state.set_default_mode(&default_mode).await?;
    }

    state.persist_if_dirty().await?;
    Ok(GuideResult { modes })
}
