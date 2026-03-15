use anyhow::{Result, bail};
use std::fmt::Write as _;

use super::super::modes::{
    ModeView, load_mode_views, provider_default_model, select_mode, supported_protocols,
    user_anthropic_base_url, user_openai_base_url,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum IntegrationTool {
    All,
    Cursor,
    Codex,
    ClaudeCode,
    Env,
    Python,
    Node,
    Curl,
    Anthropic,
}

pub(crate) fn parse_integration_tool(tool: Option<&str>) -> Result<IntegrationTool> {
    match tool.map(|tool| tool.trim().to_ascii_lowercase()) {
        None => Ok(IntegrationTool::All),
        Some(tool) if tool.is_empty() || tool == "all" => Ok(IntegrationTool::All),
        Some(tool) if tool == "cursor" => Ok(IntegrationTool::Cursor),
        Some(tool) if tool == "codex" => Ok(IntegrationTool::Codex),
        Some(tool) if tool == "claude-code" || tool == "claudecode" => {
            Ok(IntegrationTool::ClaudeCode)
        }
        Some(tool) if tool == "env" || tool == "shell" => Ok(IntegrationTool::Env),
        Some(tool) if tool == "python" => Ok(IntegrationTool::Python),
        Some(tool) if tool == "node" || tool == "javascript" => Ok(IntegrationTool::Node),
        Some(tool) if tool == "curl" => Ok(IntegrationTool::Curl),
        Some(tool) if tool == "anthropic" => Ok(IntegrationTool::Anthropic),
        Some(tool) => bail!(
            "unknown integration target '{}'; use one of: cursor, codex, claude-code, env, python, node, curl, anthropic",
            tool
        ),
    }
}

fn render_openai_tool_settings(
    out: &mut String,
    title: &str,
    base_url: &str,
    key: Option<&str>,
    model: &str,
) {
    let _ = writeln!(out, "{}:", title);
    let _ = writeln!(out, "  Base URL: {}", base_url);
    let _ = writeln!(out, "  API Key: {}", key.unwrap_or("<gateway api key>"));
    let _ = writeln!(out, "  Model: {}", model);
}

fn render_openai_env_block(out: &mut String, base_url: &str, key: Option<&str>, model: &str) {
    let _ = writeln!(out, "Shell environment:");
    let _ = writeln!(out, "  export OPENAI_BASE_URL={}", base_url);
    let _ = writeln!(
        out,
        "  export OPENAI_API_KEY={}",
        key.unwrap_or("<gateway api key>")
    );
    let _ = writeln!(out, "  export OPENAI_MODEL={}", model);
}

fn render_openai_python_block(out: &mut String, base_url: &str, key: Option<&str>, model: &str) {
    let _ = writeln!(out, "Python (openai SDK):");
    let _ = writeln!(out, "  from openai import OpenAI");
    let _ = writeln!(
        out,
        "  client = OpenAI(base_url=\"{}\", api_key=\"{}\")",
        base_url,
        key.unwrap_or("<gateway api key>")
    );
    let _ = writeln!(
        out,
        "  print(client.chat.completions.create(model=\"{}\", messages=[{{\"role\": \"user\", \"content\": \"hello\"}}]).choices[0].message.content)",
        model
    );
}

fn render_openai_node_block(out: &mut String, base_url: &str, key: Option<&str>, model: &str) {
    let _ = writeln!(out, "Node (openai SDK):");
    let _ = writeln!(out, "  import OpenAI from \"openai\";");
    let _ = writeln!(
        out,
        "  const client = new OpenAI({{ baseURL: \"{}\", apiKey: \"{}\" }});",
        base_url,
        key.unwrap_or("<gateway api key>")
    );
    let _ = writeln!(
        out,
        "  const response = await client.chat.completions.create({{ model: \"{}\", messages: [{{ role: \"user\", content: \"hello\" }}] }});",
        model
    );
    let _ = writeln!(out, "  console.log(response.choices[0].message.content);");
}

fn render_openai_curl_block(out: &mut String, base_url: &str, key: Option<&str>, model: &str) {
    let _ = writeln!(out, "curl:");
    let _ = writeln!(
        out,
        "  curl -s {}/chat/completions -H \"Authorization: Bearer {}\" -H \"Content-Type: application/json\" -d '{{\"model\":\"{}\",\"messages\":[{{\"role\":\"user\",\"content\":\"hello\"}}]}}'",
        base_url,
        key.unwrap_or("<gateway api key>"),
        model
    );
}

fn render_anthropic_block(out: &mut String, base_url: &str, key: Option<&str>, model: &str) {
    let _ = writeln!(out, "Anthropic-compatible clients:");
    let _ = writeln!(out, "  Base URL: {}", base_url);
    let _ = writeln!(out, "  x-api-key: {}", key.unwrap_or("<gateway api key>"));
    let _ = writeln!(out, "  Model: {}", model);
    let _ = writeln!(out, "  curl:");
    let _ = writeln!(
        out,
        "    curl -s {}/v1/messages -H \"x-api-key: {}\" -H \"anthropic-version: 2023-06-01\" -H \"Content-Type: application/json\" -d '{{\"model\":\"{}\",\"max_tokens\":64,\"messages\":[{{\"role\":\"user\",\"content\":\"hello\"}}]}}'",
        base_url,
        key.unwrap_or("<gateway api key>"),
        model
    );
}

pub(crate) fn render_integration_output_for_tool(
    mode: &ModeView,
    key: Option<&str>,
    bind_override: Option<&str>,
    tool: IntegrationTool,
) -> String {
    let openai_provider = mode
        .providers
        .iter()
        .find(|provider| provider.is_enabled && provider.provider_type == "openai");
    let anthropic_provider = mode
        .providers
        .iter()
        .find(|provider| provider.is_enabled && provider.provider_type == "anthropic");

    let mut out = String::new();
    let protocols = supported_protocols(mode);

    let _ = writeln!(&mut out, "Mode: {} ({})", mode.id, mode.name);
    let _ = writeln!(&mut out, "Routing: {}", mode.routing_strategy);
    let _ = writeln!(
        &mut out,
        "Protocols: {}",
        if protocols.is_empty() {
            "none".to_string()
        } else {
            protocols.join(", ")
        }
    );

    if let Some(key) = key {
        let _ = writeln!(&mut out, "Gateway API Key: {}", key);
    } else {
        let _ = writeln!(
            &mut out,
            "Gateway API Key: <create one with ug create-api-key>"
        );
    }

    if let Some(provider) = openai_provider {
        let model = provider_default_model(provider, "your-model");
        let base_url = user_openai_base_url(bind_override);
        let _ = writeln!(&mut out);
        let wants_openai = matches!(
            tool,
            IntegrationTool::All
                | IntegrationTool::Cursor
                | IntegrationTool::Codex
                | IntegrationTool::ClaudeCode
                | IntegrationTool::Env
                | IntegrationTool::Python
                | IntegrationTool::Node
                | IntegrationTool::Curl
        );

        if wants_openai {
            let _ = writeln!(&mut out, "OpenAI-compatible integrations:");
            match tool {
                IntegrationTool::All => {
                    render_openai_tool_settings(
                        &mut out,
                        "  Cursor (OpenAI-compatible provider)",
                        &base_url,
                        key,
                        model,
                    );
                    let _ = writeln!(&mut out);
                    render_openai_tool_settings(
                        &mut out,
                        "  Codex / codex-cli",
                        &base_url,
                        key,
                        model,
                    );
                    let _ = writeln!(&mut out);
                    render_openai_tool_settings(
                        &mut out,
                        "  Claude Code custom OpenAI endpoint",
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
                IntegrationTool::Cursor => render_openai_tool_settings(
                    &mut out,
                    "  Cursor (OpenAI-compatible provider)",
                    &base_url,
                    key,
                    model,
                ),
                IntegrationTool::Codex => render_openai_tool_settings(
                    &mut out,
                    "  Codex / codex-cli",
                    &base_url,
                    key,
                    model,
                ),
                IntegrationTool::ClaudeCode => render_openai_tool_settings(
                    &mut out,
                    "  Claude Code custom OpenAI endpoint",
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
        IntegrationTool::Cursor
            | IntegrationTool::Codex
            | IntegrationTool::ClaudeCode
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

    if let Some(provider) = anthropic_provider {
        let model = provider_default_model(provider, "your-model");
        let base_url = user_anthropic_base_url(bind_override);
        if matches!(tool, IntegrationTool::All | IntegrationTool::Anthropic) {
            let _ = writeln!(&mut out);
            render_anthropic_block(&mut out, &base_url, key, model);
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
        render_integration_output_for_tool(mode, key.as_deref(), bind_override, tool)
    );
    Ok(())
}
