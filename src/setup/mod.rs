mod prompts;
mod registry;

use anyhow::Result;
use clap::Args;
use console::style;
use dialoguer::{Confirm, Select, theme::ColorfulTheme};
use indicatif::{ProgressBar, ProgressStyle};
use std::path::Path;
use std::time::Duration;
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
                            step = 8;
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
                        step = 8;
                    }
                    SetupFlow::Back => {
                        step = 5;
                    }
                }
            }
            7 => {
                // Non-interactive finalize (skip agent selection)
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

                let default_mode = result.modes.first().map(|m| m.id.as_str()).unwrap_or("default");

                println!(
                    "\n  {} Configuration complete! Mode: {}",
                    style("✅").green(),
                    style(default_mode).cyan().bold()
                );
                println!(
                    "  Provider '{}' configured with API key.",
                    style(&provider_name).bold()
                );
                println!(
                    "\n  Start the gateway:\n    {}\n",
                    style("ug serve").cyan()
                );
                println!(
                    "  {} Run '{}' to see hints for other tools.",
                    style("💡").dim(),
                    style(format!("ug integrations --mode {}", default_mode)).cyan()
                );
                println!(
                    "  {} Run '{}' to see all available commands.",
                    style("💡").dim(),
                    style("ug help").cyan()
                );
                return Ok(());
            }
            8 => {
                // Interactive: collect agent selection BEFORE configuring
                let agent_options = vec![
                    "Claude Code",
                    "Cursor",
                    "Cline",
                    "OpenClaw",
                    "Zed",
                    "Droid",
                    "OpenCode",
                    "Codex",
                    "OpenHands",
                    "Trae",
                    "Skip",
                ];
                match Select::with_theme(&theme)
                    .with_prompt("Which AI agent do you want to configure?")
                    .items(&agent_options)
                    .default(0)
                    .interact_opt()
                    .unwrap()
                {
                    Some(sel) => {
                        let tool_name = match sel {
                            0 => Some("claude-code"),
                            1 => Some("cursor"),
                            2 => Some("cline"),
                            3 => Some("openclaw"),
                            4 => Some("zed"),
                            5 => Some("droid"),
                            6 => Some("opencode"),
                            7 => Some("codex"),
                            8 => Some("openhands"),
                            9 => Some("trae"),
                            _ => None,
                        };

                        // Show spinner while configuring
                        let spinner = ProgressBar::new_spinner();
                        spinner.set_style(
                            ProgressStyle::with_template("  {spinner} {msg}")
                                .unwrap()
                                .tick_strings(&["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"]),
                        );
                        spinner.set_message("Configuring gateway...");
                        spinner.enable_steady_tick(Duration::from_millis(80));

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

                        // Auto-start or detect running gateway
                        let already_running = cli::is_running();
                        let gateway_started = if already_running.is_some() {
                            true
                        } else {
                            cli::daemonize().is_ok()
                        };

                        // Brief pause so spinner is visible
                        tokio::time::sleep(Duration::from_millis(800)).await;
                        spinner.finish_and_clear();

                        let default_mode = result.modes.first().map(|m| m.id.as_str()).unwrap_or("default");

                        // -- All output at once --
                        println!();
                        println!(
                            "  {} Configuration complete! Mode: {}",
                            style("✅").green(),
                            style(default_mode).cyan().bold()
                        );
                        println!(
                            "  Provider '{}' configured with API key.",
                            style(&provider_name).bold()
                        );

                        if let Some(pid) = already_running {
                            println!(
                                "\n  {} Gateway is running (PID: {}). New config has been saved, but the running process has not reloaded it. Restart to apply it: {} then {}",
                                style("🟢").green(),
                                style(pid).bold(),
                                style("ug stop").cyan(),
                                style("ug serve").cyan()
                            );
                        } else if gateway_started {
                            println!(
                                "\n  {} Gateway started.",
                                style("🟢").green()
                            );
                        } else {
                            println!(
                                "\n  Start the gateway with: {}",
                                style("ug serve").cyan()
                            );
                        }

                        if let Some(tool_name) = tool_name {
                            for mode in &result.modes {
                                println!();
                                cli::print_integrations_with_key(
                                    &config,
                                    Some(&mode.id),
                                    Some(tool_name),
                                    Some(&mode.key),
                                    None,
                                )
                                .await?;
                            }
                        }

                        println!();
                        println!(
                            "  {} Run '{}' to see hints for other tools.",
                            style("💡").dim(),
                            style(format!("ug integrations --mode {}", default_mode)).cyan()
                        );
                        println!(
                            "  {} Run '{}' to see all available commands.",
                            style("💡").dim(),
                            style("ug help").cyan()
                        );
                        return Ok(());
                    }
                    None => {
                        // User pressed Esc, go back
                        if setup_type_selection == 1 {
                            step = 6;
                        } else {
                            step = 4;
                        }
                    }
                }
            }
            _ => unreachable!("guide step out of range"),
        }
    }
}
