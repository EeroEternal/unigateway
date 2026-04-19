pub mod diagnostics;
pub mod guide;
pub mod modes;
pub mod process;
pub mod render;
mod setup;
mod setup_prompts;
mod setup_registry;
#[cfg(test)]
mod tests;

use anyhow::Result;
use clap::{Args, Parser, Subcommand};
use std::path::Path;
use unigateway_config::GatewayState;

pub use clap_complete::Shell;

pub fn default_config_path() -> String {
    let dir = dirs::config_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("unigateway");
    dir.join("config.toml").to_string_lossy().into_owned()
}

pub(crate) fn bind_from_env() -> String {
    std::env::var("UNIGATEWAY_BIND").unwrap_or_else(|_| {
        std::env::var("PORT")
            .map(|port| format!("0.0.0.0:{port}"))
            .unwrap_or_else(|_| "127.0.0.1:3210".to_string())
    })
}

#[derive(Parser, Debug)]
#[command(name = "ug", version, about = "UniGateway – lightweight LLM gateway")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Start the gateway server.
    #[command(
        about = "Start the gateway server",
        long_about = "Starts the UniGateway server.

Examples:
  # Start server in background (default)
  ug serve

  # Start in foreground (blocking)
  ug serve --foreground

  # Bind to a specific address
  ug serve --bind 0.0.0.0:8000"
    )]
    Serve {
        #[arg(long)]
        bind: Option<String>,
        #[arg(long)]
        config: Option<String>,
        /// Run in the foreground (blocking).
        #[arg(short, long, default_value_t = false)]
        foreground: bool,
        /// Internal flag for detached process.
        #[arg(long, hide = true)]
        detached: bool,
    },
    /// Stop the background gateway process.
    #[command(about = "Stop the background gateway process")]
    Stop,
    /// Check the status of the background gateway process.
    #[command(about = "Check the status of the background gateway process")]
    Status,
    /// View the background gateway logs.
    #[command(
        about = "View the background gateway logs",
        long_about = "View the background gateway logs.

Examples:
  # Print current logs
  ug logs

  # Follow log output (tail -f)
  ug logs --follow"
    )]
    Logs {
        /// Tail the logs.
        #[arg(short, long, default_value_t = false)]
        follow: bool,
    },
    /// Print a snapshot of current metrics.
    #[command(about = "Print a snapshot of current metrics")]
    Metrics {
        #[arg(long, default_value_t = default_config_path())]
        config: String,
    },
    /// Explore user-facing modes (semantic alias over services).
    #[command(
        alias = "models",
        about = "Explore user-facing modes",
        long_about = "Explore user-facing modes (semantic alias over services).

Examples:
  # List all modes
  ug mode list

  # Show details for a specific mode
  ug mode show default

  # Set the default mode
  ug mode use fast"
    )]
    Mode {
        #[command(subcommand)]
        action: ModeAction,
    },
    /// Explain how a mode routes requests to providers.
    #[command(
        about = "Explain how a mode routes requests to providers",
        long_about = "Explain how a mode routes requests to providers.

Examples:
  # Explain routing for the default mode
  ug route explain

  # Explain routing for a specific mode
  ug route explain --mode fast"
    )]
    Route {
        #[command(subcommand)]
        action: RouteAction,
    },
    /// Print tool integration hints for a configured mode.
    #[command(
        about = "Print tool integration hints for a configured mode",
        long_about = "Print tool integration hints for a configured mode.

Examples:
  # Show integration hints for all tools
  ug integrations

  # Show hints for Cursor
  ug integrations --tool cursor

  # Show hints for a specific mode
  ug integrations --mode fast"
    )]
    Integrations {
        #[arg(long)]
        mode: Option<String>,
        #[arg(long)]
        tool: Option<String>,
        #[arg(long)]
        bind: Option<String>,
        #[arg(long, default_value_t = default_config_path())]
        config: String,
    },
    /// Interactive launch/setup for AI tools.
    #[command(
        about = "Interactive launch/setup for AI tools",
        long_about = "Interactive launch/setup for AI tools.

Examples:
  # Launch the interactive tool picker
  ug launch

  # Directly show setup for a tool
  ug launch claudecode"
    )]
    Launch {
        /// Optional tool name to bypass picker
        tool: Option<String>,
        #[arg(long)]
        mode: Option<String>,
        #[arg(long)]
        bind: Option<String>,
        #[arg(long, default_value_t = default_config_path())]
        config: String,
    },
    /// Run a smoke test against the local gateway for a mode.
    #[command(
        about = "Run a smoke test against the local gateway for a mode",
        long_about = "Run a smoke test against the local gateway for a mode.

Examples:
  # Test the default mode
  ug test

  # Test a specific mode
  ug test --mode fast

  # Test using Anthropic protocol
  ug test --protocol anthropic"
    )]
    Test {
        #[arg(long)]
        mode: Option<String>,
        #[arg(long)]
        protocol: Option<String>,
        #[arg(long)]
        bind: Option<String>,
        #[arg(long, default_value_t = default_config_path())]
        config: String,
    },
    /// Inspect current config and local gateway readiness.
    #[command(about = "Inspect current config and local gateway readiness")]
    Doctor {
        #[arg(long)]
        mode: Option<String>,
        #[arg(long)]
        bind: Option<String>,
        #[arg(long, default_value_t = default_config_path())]
        config: String,
    },
    /// Manage services (logical groupings of models).
    #[command(
        about = "Manage services (logical groupings of models)",
        long_about = "Manage services (logical groupings of models).

Examples:
  # List services
  ug service list

  # Create a new service
  ug service create --id my-service --name \"My Service\""
    )]
    Service {
        #[command(subcommand)]
        action: ServiceAction,
    },
    /// Manage LLM providers.
    #[command(
        about = "Manage LLM providers",
        long_about = "Manage LLM providers.
    
Examples:
  # List all providers
  ug provider list

  # Create a new OpenAI provider
  ug provider create --name deepseek --provider-type openai --base-url https://api.deepseek.com --api-key sk-xxx --endpoint-id deepseek-chat

  # Bind a provider to a service
  ug provider bind --service-id default --provider-id 1"
    )]
    Provider {
        #[command(subcommand)]
        action: ProviderAction,
    },
    /// Manage API keys.
    #[command(
        about = "Manage API keys",
        long_about = "Manage API keys.

Examples:
  # Create a new API key
  ug key create --key my-key --service-id default

  # Create a key with quota limit
  ug key create --key limited-key --service-id default --quota-limit 1000"
    )]
    Key {
        #[command(subcommand)]
        action: KeyAction,
    },
    /// Show, edit, or locate the config file.
    #[command(about = "Show, edit, or locate the config file")]
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },
    /// Start as an MCP (Model Context Protocol) server over stdio.
    #[command(about = "Start as an MCP (Model Context Protocol) server over stdio")]
    Mcp {
        #[arg(long, default_value_t = default_config_path())]
        config: String,
    },
    /// Self-upgrade to the latest release.
    #[command(about = "Self-upgrade to the latest release")]
    Upgrade,
    /// Interactive setup guide: create service, provider, bind, and API key.
    #[command(alias = "quickstart", about = "Interactive setup guide")]
    Guide(Box<GuideCommand>),
    /// Generate shell completion scripts.
    #[command(
        about = "Generate shell completion scripts",
        long_about = "Generate shell completion scripts.

Examples:
  # Generate Zsh completion
  ug completion zsh > _ug

  # Generate Bash completion
  ug completion bash > /etc/bash_completion.d/ug"
    )]
    Completion {
        #[arg(value_enum)]
        shell: Shell,
    },
}

#[derive(Subcommand, Debug)]
pub enum ServiceAction {
    /// List all configured services.
    List {
        #[arg(long, default_value_t = default_config_path())]
        config: String,
        #[arg(long)]
        json: bool,
    },
    /// Create a new service.
    Create {
        #[arg(long)]
        id: Option<String>,
        #[arg(long)]
        name: Option<String>,
        #[arg(long, default_value_t = default_config_path())]
        config: String,
    },
}

#[derive(Subcommand, Debug)]
pub enum ProviderAction {
    /// List all registered providers.
    List {
        #[arg(long, default_value_t = default_config_path())]
        config: String,
        #[arg(long)]
        json: bool,
    },
    /// Register a new provider.
    Create {
        #[arg(long)]
        name: Option<String>,
        #[arg(long)]
        provider_type: Option<String>,
        #[arg(long)]
        endpoint_id: Option<String>,
        #[arg(long)]
        base_url: Option<String>,
        #[arg(long)]
        api_key: Option<String>,
        #[arg(long)]
        model_mapping: Option<String>,
        #[arg(long, default_value_t = default_config_path())]
        config: String,
    },
    /// Bind a provider to a service.
    Bind {
        #[arg(long)]
        service_id: String,
        #[arg(long)]
        provider_id: i64,
        #[arg(long, default_value_t = default_config_path())]
        config: String,
    },
}

#[derive(Subcommand, Debug)]
pub enum KeyAction {
    /// Create a new API key.
    Create {
        #[arg(long)]
        key: Option<String>,
        #[arg(long)]
        service_id: Option<String>,
        #[arg(long)]
        quota_limit: Option<i64>,
        #[arg(long)]
        qps_limit: Option<f64>,
        #[arg(long)]
        concurrency_limit: Option<i64>,
        #[arg(long, default_value_t = default_config_path())]
        config: String,
    },
}

#[derive(Subcommand, Debug)]
pub enum ConfigAction {
    /// Print the config file path
    Path,
    /// Print the current config contents
    Show {
        #[arg(long, default_value_t = default_config_path())]
        config: String,
    },
    /// Open the config file in $EDITOR
    Edit {
        #[arg(long, default_value_t = default_config_path())]
        config: String,
    },
    /// Get a config value.
    Get {
        key: String,
        #[arg(long, default_value_t = default_config_path())]
        config: String,
    },
    /// Set a config value.
    Set {
        key: String,
        value: String,
        #[arg(long, default_value_t = default_config_path())]
        config: String,
    },
}

#[derive(Subcommand, Debug)]
pub enum ModeAction {
    /// List all configured modes.
    List {
        #[arg(long, default_value_t = default_config_path())]
        config: String,
        #[arg(long)]
        json: bool,
    },
    /// Show providers and keys for a mode.
    Show {
        mode: String,
        #[arg(long, default_value_t = default_config_path())]
        config: String,
        #[arg(long)]
        json: bool,
    },
    /// Set the default mode used by commands that omit --mode.
    Use {
        mode: String,
        #[arg(long, default_value_t = default_config_path())]
        config: String,
    },
}

#[derive(Subcommand, Debug)]
pub enum RouteAction {
    /// Explain provider selection for a mode.
    Explain {
        mode: Option<String>,
        #[arg(long, default_value_t = default_config_path())]
        config: String,
    },
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
    #[arg(long, default_value_t = default_config_path())]
    pub config: String,
}

pub use diagnostics::{doctor, summarize_response_text, test_mode};
pub use guide::{
    GuideParams, bind_provider, create_api_key, create_provider, create_service, guide,
    interactive_create_api_key, interactive_create_provider, interactive_create_service,
    list_providers, list_services, planned_modes,
};
pub use modes::{effective_default_mode_id, list_modes, show_mode, use_mode, user_bind_address};
pub use process::{daemonize, is_running, status_server, stop_server, view_logs};
pub use render::{
    integrations::{
        IntegrationTool, interactive_launch, parse_integration_tool, print_integrations,
        print_integrations_with_key, render_integration_output_for_tool,
    },
    routes::{explain_route, render_route_explanation},
};
pub use setup::run_guide;
pub use unigateway_config::{ModeKey, ModeProvider, ModeView};

pub async fn config_get(config_path: &str, key: &str) -> Result<()> {
    let state = GatewayState::load(Path::new(config_path)).await?;
    let value = state.get_config_value(key).await?;
    println!("{}", value);
    Ok(())
}

pub async fn config_set(config_path: &str, key: &str, value: &str) -> Result<()> {
    let state = GatewayState::load(Path::new(config_path)).await?;
    state.set_config_value(key, value).await?;
    state.persist_if_dirty().await?;
    println!("set '{}' to '{}'", key, value);
    Ok(())
}

pub async fn print_metrics_snapshot(config_path: &str) -> Result<()> {
    let state = GatewayState::load(Path::new(config_path)).await?;
    let (total, openai_total, anthropic_total, embeddings_total) = state.metrics_snapshot().await;
    println!("unigateway_requests_total {}", total);
    println!(
        "unigateway_requests_by_endpoint_total{{endpoint=\"/v1/chat/completions\"}} {}",
        openai_total
    );
    println!(
        "unigateway_requests_by_endpoint_total{{endpoint=\"/v1/messages\"}} {}",
        anthropic_total
    );
    println!(
        "unigateway_requests_by_endpoint_total{{endpoint=\"/v1/embeddings\"}} {}",
        embeddings_total
    );
    Ok(())
}
