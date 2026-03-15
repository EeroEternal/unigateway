mod prompts;
mod registry;

use anyhow::Result;
use clap::Args;
use dialoguer::{Confirm, theme::ColorfulTheme};

use crate::cli;

use self::prompts::{ProviderPromptLabels, ProviderSetupInput, resolve_provider_setup};

fn config_default() -> String {
    crate::types::default_config_path()
}

#[derive(Args, Debug)]
pub struct QuickstartCommand {
    #[arg(long)]
    pub service_id: Option<String>,
    #[arg(long)]
    pub service_name: Option<String>,
    #[arg(long)]
    pub provider_name: Option<String>,
    #[arg(long)]
    pub provider_type: Option<String>,
    #[arg(long)]
    pub endpoint_id: Option<String>,
    #[arg(long)]
    pub base_url: Option<String>,
    #[arg(long)]
    pub api_key: Option<String>,
    #[arg(long)]
    pub model_mapping: Option<String>,
    #[arg(long)]
    pub backup_provider_name: Option<String>,
    #[arg(long)]
    pub backup_provider_type: Option<String>,
    #[arg(long)]
    pub backup_endpoint_id: Option<String>,
    #[arg(long)]
    pub backup_base_url: Option<String>,
    #[arg(long)]
    pub backup_api_key: Option<String>,
    #[arg(long)]
    pub backup_model_mapping: Option<String>,
    #[arg(long, default_value_t = config_default())]
    pub config: String,
}

pub async fn run_quickstart(command: QuickstartCommand) -> Result<()> {
    let QuickstartCommand {
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
    } = command;

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
                    .unwrap_or_else(|| format!("{}-backup", backup_provider_setup.provider_type))
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

    let provider_name = provider_name.unwrap_or_else(|| provider_setup.provider_type.clone());

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
            backup_default_model: backup_setup.as_ref().and_then(|setup| setup.3.as_deref()),
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
        cli::print_integrations_with_key(&config, Some(&mode.id), None, Some(&mode.key), None)
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
