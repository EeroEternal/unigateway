use anyhow::{Result, bail};
use console::style;
use dialoguer::{Select, theme::ColorfulTheme};
use std::fmt::Write as _;

use super::super::modes::{
    ModeView, load_mode_views, mode_providers_for, select_mode, supported_protocols,
    user_anthropic_base_url, user_bind_address, user_openai_base_url,
};
use crate::types::AppConfig;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum IntegrationTool {
    All,
    OpenClaw,
    Zed,
    Cursor,
    ClaudeCode,
    Droid,
    OpenCode,
    Codex,
    Cline,
    OpenHands,
    Trae,
    Env,
    Python,
    Node,
    Curl,
    Anthropic,
}

const DEFAULT_CONTEXT_WINDOW_HINT: u32 = 128_000;
const DEFAULT_MAX_OUTPUT_TOKENS_HINT: u32 = 16_384;

pub(crate) fn parse_integration_tool(tool: Option<&str>) -> Result<IntegrationTool> {
    match tool.map(|tool| tool.trim().to_ascii_lowercase()) {
        None => Ok(IntegrationTool::All),
        Some(tool) if tool.is_empty() || tool == "all" => Ok(IntegrationTool::All),
        Some(tool) if tool == "openclaw" || tool == "open-claw" => Ok(IntegrationTool::OpenClaw),
        Some(tool) if tool == "zed" => Ok(IntegrationTool::Zed),
        Some(tool) if tool == "cursor" => Ok(IntegrationTool::Cursor),
        Some(tool) if tool == "codex" => Ok(IntegrationTool::Codex),
        Some(tool) if tool == "claude-code" || tool == "claudecode" => {
            Ok(IntegrationTool::ClaudeCode)
        }
        Some(tool) if tool == "droid" || tool == "factory" => Ok(IntegrationTool::Droid),
        Some(tool) if tool == "opencode" || tool == "open-code" => Ok(IntegrationTool::OpenCode),
        Some(tool) if tool == "cline" => Ok(IntegrationTool::Cline),
        Some(tool) if tool == "openhands" || tool == "open-hands" => Ok(IntegrationTool::OpenHands),
        Some(tool) if tool == "trae" => Ok(IntegrationTool::Trae),
        Some(tool) if tool == "env" || tool == "shell" => Ok(IntegrationTool::Env),
        Some(tool) if tool == "python" => Ok(IntegrationTool::Python),
        Some(tool) if tool == "node" || tool == "javascript" => Ok(IntegrationTool::Node),
        Some(tool) if tool == "curl" => Ok(IntegrationTool::Curl),
        Some(tool) if tool == "anthropic" => Ok(IntegrationTool::Anthropic),
        Some(tool) => bail!(
            "unknown integration target '{}'; use one of: openclaw, zed, cursor, claude-code, droid, opencode, codex, cline, openhands, trae, env, python, node, curl, anthropic",
            tool
        ),
    }
}

fn render_anthropic_block(out: &mut String, base_url: &str, key: Option<&str>, model: &str) {
    let _ = writeln!(out, "anthropic-compatible (e.g. cursor, claude dev):");
    let _ = writeln!(out, "  base url: {}", style(base_url).cyan());
    let _ = writeln!(out, "  api key:  {}", style(key.unwrap_or("<gateway api key>")).cyan());
    let _ = writeln!(out, "  model:    {}", style(model).cyan());
}

fn render_claude_code_block(out: &mut String, base_url: &str, key: Option<&str>, model: &str) {
    let key = key.unwrap_or("<gateway api key>");
    let _ = writeln!(out, "claude code:");
    let _ = writeln!(
        out,
        "  {}",
        style(format!("export ANTHROPIC_BASE_URL={}", base_url)).cyan()
    );
    let _ = writeln!(
        out,
        "  {}",
        style(format!("export ANTHROPIC_API_KEY={}", key)).cyan()
    );
    let _ = writeln!(out, "  {}", style(format!("export ANTHROPIC_MODEL={}", model)).cyan());
    let _ = writeln!(out);
    let _ = writeln!(out, "  launch:");
    let _ = writeln!(
        out,
        "  {}",
        style(format!(
            "ANTHROPIC_BASE_URL={} ANTHROPIC_API_KEY={} ANTHROPIC_MODEL={} claude",
            base_url, key, model
        ))
        .cyan()
    );
}

fn render_openai_tool_settings(
    out: &mut String,
    title: &str,
    base_url: &str,
    key: Option<&str>,
    model: &str,
) {
    let _ = writeln!(out, "{}:", title.to_lowercase());
    let _ = writeln!(out, "  base url: {}", style(base_url).cyan());
    let _ = writeln!(out, "  api key:  {}", style(key.unwrap_or("<gateway api key>")).cyan());
    let _ = writeln!(out, "  model:    {}", style(model).cyan());
}

fn render_codex_block(out: &mut String, base_url: &str, key: Option<&str>, model: &str) {
    let api_key = key.unwrap_or("<gateway api key>");
    let _ = writeln!(out, "codex / codex-cli:");
    let _ = writeln!(out, "  base url: {}", style(base_url).cyan());
    let _ = writeln!(out, "  api key:  {}", style(api_key).cyan());
    let _ = writeln!(out, "  model:    {}", style(model).cyan());
    let _ = writeln!(out);
    let _ = writeln!(out, "  launch:");
    let _ = writeln!(out, "  {}", style(format!("export OPENAI_BASE_URL={}", base_url)).cyan());
    let _ = writeln!(out, "  {}", style(format!("export OPENAI_API_KEY={}", api_key)).cyan());
    let _ = writeln!(out, "  {}", style(format!("export OPENAI_MODEL={}", model)).cyan());
}

fn render_openai_env_block(out: &mut String, base_url: &str, key: Option<&str>, model: &str) {
    let _ = writeln!(out, "shell environment:");
    let _ = writeln!(out, "  {}", style(format!("export OPENAI_BASE_URL={}", base_url)).cyan());
    let _ = writeln!(
        out,
        "  {}",
        style(format!("export OPENAI_API_KEY={}", key.unwrap_or("<gateway api key>"))).cyan()
    );
    let _ = writeln!(out, "  {}", style(format!("export OPENAI_MODEL={}", model)).cyan());
}

fn render_openai_python_block(out: &mut String, base_url: &str, key: Option<&str>, model: &str) {
    let _ = writeln!(out, "Python (openai SDK):");
    let code = format!(
        r#"  from openai import OpenAI
  client = OpenAI(base_url="{}", api_key="{}")
  print(client.chat.completions.create(model="{}", messages=[{{"role": "user", "content": "hello"}}]).choices[0].message.content)"#,
        base_url,
        key.unwrap_or("<gateway api key>"),
        model
    );
    let _ = writeln!(out, "{}", style(code).dim());
}

fn render_openai_node_block(out: &mut String, base_url: &str, key: Option<&str>, model: &str) {
    let _ = writeln!(out, "Node (openai SDK):");
    let code = format!(
        r#"  import OpenAI from "openai";
  const client = new OpenAI({{ baseURL: "{}", apiKey: "{}" }});
  const response = await client.chat.completions.create({{ model: "{}", messages: [{{ role: "user", content: "hello" }}] }});
  console.log(response.choices[0].message.content);"#,
        base_url,
        key.unwrap_or("<gateway api key>"),
        model
    );
    let _ = writeln!(out, "{}", style(code).dim());
}

fn render_openai_curl_block(out: &mut String, base_url: &str, key: Option<&str>, model: &str) {
    let _ = writeln!(out, "curl:");
    let cmd = format!(
        r#"  curl -s {}/chat/completions -H "Authorization: Bearer {}" -H "Content-Type: application/json" -d '{{"model":"{}","messages":[{{"role":"user","content":"hello"}}]}}'"#,
        base_url,
        key.unwrap_or("<gateway api key>"),
        model
    );
    let _ = writeln!(out, "{}", style(cmd).dim());
}

fn render_openclaw_block(out: &mut String, base_url: &str, key: Option<&str>, model: &str) {
    let _ = writeln!(out, "OpenClaw (~/.openclaw/openclaw.json):");
    let config = serde_json::json!({
        "agents": {
            "defaults": {
                "model": {
                    "primary": format!("unigateway/{}", model)
                }
            }
        },
        "models": {
            "mode": "merge",
            "providers": {
                "unigateway": {
                    "baseUrl": base_url,
                    "apiKey": "${UNIGATEWAY_API_KEY}",
                    "api": "openai-completions",
                    "models": [
                        {
                            "id": model,
                            "name": format!("UniGateway {}", model)
                        }
                    ]
                }
            }
        }
    });
    let _ = writeln!(out, "{}", style(serde_json::to_string_pretty(&config).unwrap()).dim());
    if let Some(k) = key {
        let _ = writeln!(out, "  {}", style(format!("export UNIGATEWAY_API_KEY={}", k)).cyan());
    }
}

fn render_zed_block(out: &mut String, base_url: &str, key: Option<&str>, model: &str) {
    let _ = writeln!(out, "Zed (settings.json or Agent Panel > Add Provider):");
    let config = serde_json::json!({
        "language_models": {
            "openai_compatible": {
                "UniGateway": {
                    "api_url": base_url,
                    "available_models": [
                        {
                            "name": model,
                            "display_name": format!("UniGateway {}", model),
                            "max_tokens": DEFAULT_CONTEXT_WINDOW_HINT,
                            "capabilities": {
                                "tools": true,
                                "chat_completions": true,
                            },
                        }
                    ]
                }
            }
        }
    });
    let _ = writeln!(out, "{}", style(serde_json::to_string_pretty(&config).unwrap()).dim());
    if let Some(k) = key {
        let _ = writeln!(out, "  {}", style(format!("export UNIGATEWAY_API_KEY={}", k)).cyan());
    }
}

fn render_droid_block(out: &mut String, base_url: &str, key: Option<&str>, model: &str) {
    let _ = writeln!(out, "Droid (~/.factory/settings.json):");
    let config = serde_json::json!({
        "customModels": [
            {
                "model": model,
                "displayName": format!("UniGateway {}", model),
                "baseUrl": base_url,
                "apiKey": "${UNIGATEWAY_API_KEY}",
                "provider": "generic-chat-completion-api",
                "maxOutputTokens": DEFAULT_MAX_OUTPUT_TOKENS_HINT,
            }
        ]
    });
    let _ = writeln!(out, "{}", style(serde_json::to_string_pretty(&config).unwrap()).dim());
    if let Some(k) = key {
        let _ = writeln!(out, "  {}", style(format!("export UNIGATEWAY_API_KEY={}", k)).cyan());
    }
}

fn render_opencode_block(out: &mut String, base_url: &str, key: Option<&str>, model: &str) {
    let _ = writeln!(out, "OpenCode (opencode.json):");
    let config = serde_json::json!({
        "$schema": "https://opencode.ai/config.json",
        "provider": {
            "unigateway": {
                "npm": "@ai-sdk/openai-compatible",
                "name": "UniGateway",
                "options": {
                    "baseURL": base_url,
                    "apiKey": "{env:UNIGATEWAY_API_KEY}"
                },
                "models": {
                    model: {
                        "name": format!("UniGateway {}", model),
                        "limit": {
                            "context": DEFAULT_CONTEXT_WINDOW_HINT,
                            "output": DEFAULT_MAX_OUTPUT_TOKENS_HINT,
                        }
                    }
                }
            }
        }
    });
    let _ = writeln!(out, "{}", style(serde_json::to_string_pretty(&config).unwrap()).dim());
    let _ = writeln!(out, "  Then run `/connect` -> Other -> unigateway");
    if let Some(k) = key {
        let _ = writeln!(out, "  {}", style(format!("export UNIGATEWAY_API_KEY={}", k)).cyan());
    }
}

fn render_cline_block(out: &mut String, base_url: &str, key: Option<&str>, model: &str) {
    let _ = writeln!(out, "Cline (VS Code Extension):");
    let _ = writeln!(out, "  1. Open Cline settings");
    let _ = writeln!(out, "  2. Select API Provider: OpenAI Compatible");
    let _ = writeln!(out, "  3. Set Base URL: {}", style(base_url).cyan());
    let _ = writeln!(out, "  4. Set API Key: {}", style(key.unwrap_or("<gateway api key>")).cyan());
    let _ = writeln!(out, "  5. Model ID: {}", style(model).cyan());
}

fn render_openhands_block(out: &mut String, base_url: &str, key: Option<&str>, model: &str) {
    let _ = writeln!(out, "OpenHands (env or config.toml):");
    let _ = writeln!(out, "  {}", style(format!("LLM_BASE_URL=\"{}\"", base_url)).cyan());
    let _ = writeln!(out, "  {}", style(format!("LLM_API_KEY=\"{}\"", key.unwrap_or("<gateway api key>"))).cyan());
    let _ = writeln!(out, "  {}", style(format!("LLM_MODEL=\"{}\"", model)).cyan());
}

pub(crate) fn render_integration_output_for_tool(
    mode: Option<&ModeView>,
    key: Option<&str>,
    bind_override: Option<&str>,
    tool: IntegrationTool,
) -> String {
    let mut out = String::new();

    let bind_addr = match bind_override {
        Some(b) => user_bind_address(b),
        None => user_bind_address(&AppConfig::from_env().bind),
    };
    let _base_url = format!("http://{}/v1", bind_addr);

    let default_model = if let Some(mode) = mode {
        let providers = mode_providers_for(mode, "openai");
        let provider = providers.first();
        provider.and_then(|p| p.default_model.clone()).unwrap_or_else(|| "default".to_string())
    } else {
        "default".to_string()
    };
    let model = default_model.as_str();

    if let Some(key) = key {
        let _ = writeln!(&mut out, "api key: {}", key);
    } else {
        let _ = writeln!(
            &mut out,
            "api key: <none> (create with ug create-api-key)"
        );
    }

    let openai_provider = mode.and_then(|m| {
        m.providers
            .iter()
            .find(|provider| provider.is_enabled && provider.provider_type == "openai")
    });

    let anthropic_provider = mode.and_then(|m| {
        m.providers
            .iter()
            .find(|provider| provider.is_enabled && provider.provider_type == "anthropic")
    });

    if let Some(mode) = mode {
        let protocols = supported_protocols(mode);
        let _ = writeln!(&mut out, "mode:    {} ({})", mode.id, mode.name);
        let _ = writeln!(&mut out, "routing: {}", mode.routing_strategy);
        let _ = writeln!(
            &mut out,
            "proto:   {}",
            if protocols.is_empty() {
                "none".to_string()
            } else {
                protocols.join(", ")
            }
        );
    }

    if mode.is_none() || openai_provider.is_some() {
        let base_url = user_openai_base_url(bind_override);
        let _ = writeln!(&mut out);
        let wants_openai = matches!(
            tool,
            IntegrationTool::All
                | IntegrationTool::OpenClaw
                | IntegrationTool::Zed
                | IntegrationTool::Droid
                | IntegrationTool::OpenCode
                | IntegrationTool::Cursor
                | IntegrationTool::Codex
                | IntegrationTool::ClaudeCode
                | IntegrationTool::Cline
                | IntegrationTool::OpenHands
                | IntegrationTool::Trae
                | IntegrationTool::Env
                | IntegrationTool::Python
                | IntegrationTool::Node
                | IntegrationTool::Curl
        );

        if wants_openai {
            if tool == IntegrationTool::All {
                let _ = writeln!(&mut out, "openai-compatible integrations:");
            }
            match tool {
                IntegrationTool::All => {
                    render_openclaw_block(&mut out, &base_url, key, model);
                    let _ = writeln!(&mut out);
                    let _ = writeln!(&mut out);
                    render_openai_tool_settings(
                        &mut out,
                        "  cursor (openai-compatible provider)",
                        &base_url,
                        key,
                        model,
                    );
                    let _ = writeln!(&mut out);
                    render_opencode_block(&mut out, &base_url, key, model);
                    let _ = writeln!(&mut out);
                    render_droid_block(&mut out, &base_url, key, model);
                    let _ = writeln!(&mut out);
                    render_cline_block(&mut out, &base_url, key, model);
                    let _ = writeln!(&mut out);
                    render_openhands_block(&mut out, &base_url, key, model);
                    let _ = writeln!(&mut out);
                    render_zed_block(&mut out, &base_url, key, model);
                    let _ = writeln!(&mut out);
                    render_codex_block(&mut out, &base_url, key, model);
                    let _ = writeln!(&mut out);
                    render_openai_tool_settings(
                        &mut out,
                        "  trae configuration",
                        &base_url,
                        key,
                        model,
                    );
                    let _ = writeln!(&mut out);
                    render_openai_env_block(&mut out, &base_url, key, model);
                    let _ = writeln!(&mut out);
                    render_openai_python_block(&mut out, &base_url, key, model);
                    let _ = writeln!(&mut out);
                    render_openai_node_block(&mut out, &base_url, key, model);
                    let _ = writeln!(&mut out);
                    render_openai_curl_block(&mut out, &base_url, key, model);
                }
                IntegrationTool::OpenClaw => render_openclaw_block(&mut out, &base_url, key, model),
                IntegrationTool::Zed => render_zed_block(&mut out, &base_url, key, model),
                IntegrationTool::Cursor => render_openai_tool_settings(
                    &mut out,
                    "  cursor (openai-compatible provider)",
                    &base_url,
                    key,
                    model,
                ),
                IntegrationTool::Codex => render_codex_block(&mut out, &base_url, key, model),
                IntegrationTool::ClaudeCode => {
                    let anthropic_base_url = user_anthropic_base_url(bind_override);
                    render_claude_code_block(&mut out, &anthropic_base_url, key, model)
                }
                IntegrationTool::Droid => render_droid_block(&mut out, &base_url, key, model),
                IntegrationTool::OpenCode => render_opencode_block(&mut out, &base_url, key, model),
                IntegrationTool::Cline => render_cline_block(&mut out, &base_url, key, model),
                IntegrationTool::OpenHands => render_openhands_block(&mut out, &base_url, key, model),
                IntegrationTool::Trae => render_openai_tool_settings(
                    &mut out,
                    "  trae configuration",
                    &base_url,
                    key,
                    model,
                ),
                IntegrationTool::Env => render_openai_env_block(&mut out, &base_url, key, model),
                IntegrationTool::Python => {
                    render_openai_python_block(&mut out, &base_url, key, model)
                }
                IntegrationTool::Node => render_openai_node_block(&mut out, &base_url, key, model),
                IntegrationTool::Curl => render_openai_curl_block(&mut out, &base_url, key, model),
                IntegrationTool::Anthropic => {}
            }
        }
    } else if matches!(
        tool,
        IntegrationTool::OpenClaw
            | IntegrationTool::Zed
            | IntegrationTool::Cursor
            | IntegrationTool::Droid
            | IntegrationTool::OpenCode
            | IntegrationTool::Codex
            | IntegrationTool::ClaudeCode
            | IntegrationTool::Cline
            | IntegrationTool::OpenHands
            | IntegrationTool::Trae
            | IntegrationTool::Env
            | IntegrationTool::Python
            | IntegrationTool::Node
            | IntegrationTool::Curl
    ) {
        let _ = writeln!(&mut out);
        let _ = writeln!(
            &mut out,
            "No enabled OpenAI-compatible provider is bound to this mode."
        );
    }

    if mode.is_none() || anthropic_provider.is_some() {
        let base_url = user_anthropic_base_url(bind_override);
        if matches!(tool, IntegrationTool::All | IntegrationTool::Anthropic | IntegrationTool::ClaudeCode) {
            let _ = writeln!(&mut out);
            if tool == IntegrationTool::ClaudeCode {
                render_claude_code_block(&mut out, &base_url, key, model);
            } else {
                render_anthropic_block(&mut out, &base_url, key, model);
                if tool == IntegrationTool::All {
                    let _ = writeln!(&mut out);
                    render_claude_code_block(&mut out, &base_url, key, model);
                }
            }
        }
    } else if matches!(tool, IntegrationTool::Anthropic) {
        let _ = writeln!(&mut out);
        let _ = writeln!(
            &mut out,
            "No enabled Anthropic-compatible provider is bound to this mode."
        );
    }

    out.trim_end().to_string()
}

pub async fn print_integrations(
    config_path: &str,
    mode_id: Option<&str>,
    tool: Option<&str>,
    bind_override: Option<&str>,
) -> Result<()> {
    print_integrations_with_key(config_path, mode_id, tool, None, bind_override).await
}

pub async fn print_integrations_with_key(
    config_path: &str,
    mode_id: Option<&str>,
    tool: Option<&str>,
    preferred_key: Option<&str>,
    bind_override: Option<&str>,
) -> Result<()> {
    let modes = load_mode_views(config_path).await?;
    let mode = select_mode(&modes, mode_id)?;
    let tool = parse_integration_tool(tool)?;
    let key = preferred_key.map(ToOwned::to_owned).or_else(|| {
        mode.keys
            .iter()
            .find(|key| key.is_active)
            .or_else(|| mode.keys.first())
            .map(|key| key.key.clone())
    });

    println!(
        "{}",
        render_integration_output_for_tool(Some(mode), key.as_deref(), bind_override, tool)
    );
    Ok(())
}

pub async fn interactive_launch(
    config_path: &str,
    tool: Option<String>,
    mode_id: Option<String>,
    bind_override: Option<String>,
) -> Result<()> {
    let modes = load_mode_views(config_path).await?;
    let mode = select_mode(&modes, mode_id.as_deref())?;

    let tool_choice = if let Some(t) = tool {
        parse_integration_tool(Some(&t))?
    } else {
        let tools = [
            ("Claude Code", IntegrationTool::ClaudeCode),
            ("OpenClaw", IntegrationTool::OpenClaw),
            ("Zed", IntegrationTool::Zed),
            ("Cursor", IntegrationTool::Cursor),
            ("OpenCode", IntegrationTool::OpenCode),
            ("Cline", IntegrationTool::Cline),
            ("OpenHands", IntegrationTool::OpenHands),
            ("Trae", IntegrationTool::Trae),
            ("Codex", IntegrationTool::Codex),
            ("Droid", IntegrationTool::Droid),
            ("Shell Env", IntegrationTool::Env),
            ("Python SDK", IntegrationTool::Python),
            ("Node SDK", IntegrationTool::Node),
            ("curl", IntegrationTool::Curl),
        ];

        let selection = Select::with_theme(&ColorfulTheme::default())
            .with_prompt("select a tool to launch")
            .items(&tools.iter().map(|(n, _)| *n).collect::<Vec<_>>())
            .default(0)
            .interact()?;

        tools[selection].1
    };

    let key = mode
        .keys
        .iter()
        .find(|key| key.is_active)
        .or_else(|| mode.keys.first())
        .map(|key| key.key.clone());

    println!("\n🎉 ready to use with {}!", style(format!("{:?}", tool_choice).to_lowercase()).green().bold());
    println!(
        "{}",
        render_integration_output_for_tool(Some(mode), key.as_deref(), bind_override.as_deref(), tool_choice)
    );

    Ok(())
}
