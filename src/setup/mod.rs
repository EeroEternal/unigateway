mod prompts;
mod registry;

use anyhow::Result;
use clap::Args;
use dialoguer::{Confirm, Select, theme::ColorfulTheme};
use std::path::Path;
use std::fs;

use crate::cli;

use self::prompts::{ProviderPromptLabels, ProviderSetupInput, SetupFlow, resolve_provider_setup};

fn config_default() -> String {
    crate::types::default_config_path()
}

#[derive(Args, Debug)]
pub struct GuideCommand {
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
    pub fast_model: Option<String>,
    #[arg(long)]
    pub strong_model: Option<String>,
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

pub async fn run_guide(command: GuideCommand) -> Result<()> {
    let GuideCommand {
        service_id,
        service_name,
        provider_name: provider_name_arg,
        provider_type,
        endpoint_id,
        base_url,
        api_key,
        model_mapping,
        fast_model: _,
        strong_model: _,
        backup_provider_name,
        backup_provider_type,
        backup_endpoint_id,
        backup_base_url,
        backup_api_key,
        backup_model_mapping,
        config,
    } = command;

    let interactive = provider_type.is_none() || endpoint_id.is_none() || api_key.is_none();
    let theme = ColorfulTheme::default();

    // Guide State
    let mut step = 0;
    let mut provider_setup = None;
    let mut backup_setup = None;
    let mut setup_type_selection = 0;
    let mut fast_model = None;
    let mut strong_model = None;
    let mut add_backup = false;

    loop {
        match step {
            0 => {
                // Initial Menu / Welcome
                if !interactive {
                    step = 1;
                    continue;
                }
                println!("\n  Welcome to UniGateway guide!\n");
                let config_path = Path::new(&config);
                if config_path.exists() {
                    println!("  Existing configuration found at: {}\n", config);
                    let options = [
                        "Add to existing configuration (recommended)",
                        "Show current configuration",
                        "Clear existing configuration and start fresh",
                        "Cancel guide",
                    ];
                    let mut selection = 0;
                    loop {
                        match Select::with_theme(&theme)
                            .with_prompt("How would you like to proceed?")
                            .items(&options)
                            .default(selection)
                            .interact_opt()
                            .unwrap()
                        {
                            Some(1) => {
                                let contents = fs::read_to_string(config_path)?;
                                println!(
                                    "\n--- Current Configuration ---\n{}\n-----------------------------\n",
                                    contents
                                );
                                selection = 1;
                                continue;
                            }
                            Some(2) => {
                                fs::remove_file(config_path)?;
                                println!("  Existing configuration cleared.\n");
                                step = 1;
                                break;
                            }
                            Some(3) => {
                                println!("  Guide cancelled.");
                                return Ok(());
                            }
                            Some(0) => {
                                step = 1;
                                break;
                            }
                            None => {
                                println!("  Guide cancelled.");
                                return Ok(());
                            }
                            _ => break,
                        }
                    }
                } else {
                    step = 1;
                }
            }
            1 => {
                // Primary Provider Setup
                match resolve_provider_setup(
                    ProviderPromptLabels {
                        provider: "Provider",
                        model: "Default model",
                        base_url: "Base URL",
                        api_key: "API Key",
                    },
                    ProviderSetupInput {
                        provider_type: provider_type.clone(),
                        endpoint_id: endpoint_id.clone(),
                        default_model: None,
                        base_url: base_url.clone(),
                        api_key: api_key.clone(),
                    },
                ) {
                    SetupFlow::Next(setup) => {
                        provider_setup = Some(setup);
                        step = 2;
                    }
                    SetupFlow::Back => {
                        if !interactive {
                            return Ok(());
                        }
                        step = 0;
                    }
                }
            }
            2 => {
                // Backup Confirmation
                if !interactive {
                    let backup_requested = backup_provider_name.is_some()
                        || backup_provider_type.is_some()
                        || backup_endpoint_id.is_some()
                        || backup_base_url.is_some()
                        || backup_api_key.is_some()
                        || backup_model_mapping.is_some();
                    if backup_requested {
                        step = 3;
                    } else {
                        step = 4;
                    }
                    continue;
                }

                match Confirm::with_theme(&theme)
                    .with_prompt("Add a second provider for automatic fallback?")
                    .default(add_backup)
                    .interact_opt()
                    .unwrap()
                {
                    Some(val) => {
                        add_backup = val;
                        if add_backup {
                            step = 3;
                        } else {
                            step = 4;
                        }
                    }
                    None => {
                        step = 1;
                    }
                }
            }
            3 => {
                // Backup Provider Setup
                match resolve_provider_setup(
                    ProviderPromptLabels {
                        provider: "Backup provider",
                        model: "Backup default model",
                        base_url: "Backup base URL",
                        api_key: "Backup API Key",
                    },
                    ProviderSetupInput {
                        provider_type: backup_provider_type.clone(),
                        endpoint_id: backup_endpoint_id.clone(),
                        default_model: None,
                        base_url: backup_base_url.clone(),
                        api_key: backup_api_key.clone(),
                    },
                ) {
                    SetupFlow::Next(setup) => {
                        let b_name = backup_provider_name
                            .clone()
                            .unwrap_or_else(|| format!("{}-backup", setup.name));
                        backup_setup = Some((
                            b_name,
                            setup.provider_type,
                            setup.endpoint_id,
                            setup.default_model,
                            setup.base_url,
                            setup.api_key,
                        ));
                        step = 4;
                    }
                    SetupFlow::Back => {
                        if !interactive {
                            step = 2;
                        } else {
                            step = 2;
                        }
                    }
                }
            }
            4 => {
                // Setup Type Selection (Simple vs Multi-model)
                if !interactive {
                    step = 7;
                    continue;
                }
                let options = [
                    "Simple (single `default` mode)",
                    "Multi-model (separate `fast` and `strong` modes)",
                ];
                match Select::with_theme(&theme)
                    .with_prompt("Choose setup type")
                    .items(&options)
                    .default(setup_type_selection)
                    .interact_opt()
                    .unwrap()
                {
                    Some(selection) => {
                        setup_type_selection = selection;
                        if selection == 1 {
                            println!("\n  Choose models for multi-model setup:");
                            step = 5;
                        } else {
                            step = 7;
                        }
                    }
                    None => {
                        if add_backup {
                            step = 3;
                        } else {
                            step = 2;
                        }
                    }
                }
            }
            5 => {
                // Multi-model (Fast model)
                let p_setup = provider_setup.as_ref().unwrap();
                match resolve_provider_setup(
                    ProviderPromptLabels {
                        provider: "Provider (Fast)",
                        model: "Fast model",
                        base_url: "Base URL",
                        api_key: "API Key",
                    },
                    ProviderSetupInput {
                        provider_type: Some(p_setup.provider_type.clone()),
                        endpoint_id: Some(p_setup.endpoint_id.clone()),
                        default_model: None,
                        base_url: p_setup.base_url.clone(),
                        api_key: Some(p_setup.api_key.clone()),
                    },
                ) {
                    SetupFlow::Next(setup) => {
                        fast_model = setup.default_model;
                        step = 6;
                    }
                    SetupFlow::Back => {
                        step = 4;
                    }
                }
            }
            6 => {
                // Multi-model (Strong model)
                let p_setup = provider_setup.as_ref().unwrap();
                match resolve_provider_setup(
                    ProviderPromptLabels {
                        provider: "Provider (Strong)",
                        model: "Strong model",
                        base_url: "Base URL",
                        api_key: "API Key",
                    },
                    ProviderSetupInput {
                        provider_type: Some(p_setup.provider_type.clone()),
                        endpoint_id: Some(p_setup.endpoint_id.clone()),
                        default_model: None,
                        base_url: p_setup.base_url.clone(),
                        api_key: Some(p_setup.api_key.clone()),
                    },
                ) {
                    SetupFlow::Next(setup) => {
                        strong_model = setup.default_model;
                        step = 7;
                    }
                    SetupFlow::Back => {
                        step = 5;
                    }
                }
            }
            7 => {
                // Finalize and Save
                let provider_setup = provider_setup.unwrap();
                let provider_name = provider_name_arg
                    .clone()
                    .unwrap_or_else(|| provider_setup.name.clone());

                let result = cli::guide(
                    &config,
                    cli::GuideParams {
                        service_id: service_id.as_deref(),
                        service_name: service_name.as_deref(),
                        provider_name: &provider_name,
                        provider_type: &provider_setup.provider_type,
                        endpoint_id: &provider_setup.endpoint_id,
                        default_model: provider_setup.default_model.as_deref(),
                        fast_model: fast_model.as_deref().or(command.fast_model.as_deref()),
                        strong_model: strong_model.as_deref().or(command.strong_model.as_deref()),
                        base_url: provider_setup.base_url.as_deref(),
                        api_key: &provider_setup.api_key,
                        model_mapping: model_mapping.as_deref(),
                        backup_provider_name: backup_setup.as_ref().map(|s| s.0.as_str()),
                        backup_provider_type: backup_setup.as_ref().map(|s| s.1.as_str()),
                        backup_endpoint_id: backup_setup.as_ref().map(|s| s.2.as_str()),
                        backup_default_model: backup_setup.as_ref().and_then(|s| s.3.as_deref()),
                        backup_base_url: backup_setup.as_ref().and_then(|s| s.4.as_deref()),
                        backup_api_key: backup_setup.as_ref().map(|s| s.5.as_str()),
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
                println!(
                    "  ✓ Provider '{}' configured with API key.\n",
                    provider_name
                );
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
                    println!("    ug integrations --mode {} --tool openclaw", mode.id);
                    println!("    ug integrations --mode {} --tool zed", mode.id);
                    println!("    ug integrations --mode {} --tool claude-code", mode.id);
                    println!("    ug integrations --mode {} --tool cursor\n", mode.id);
                }

                println!("  You can reprint these hints later with:\n");
                println!("    ug integrations --mode <mode>");
                println!(
                    "    ug integrations --mode <mode> --tool <openclaw|zed|cursor|claude-code|droid|opencode|codex|env|python|node|curl|anthropic>\n"
                );
                return Ok(());
            }
            _ => unreachable!("guide step out of range"),
        }
    }
}
