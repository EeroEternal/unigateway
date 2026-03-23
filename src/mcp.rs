use std::path::Path;
use std::sync::Arc;

use rmcp::handler::server::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::*;
use rmcp::transport::stdio;
use rmcp::{ServerHandler, ServiceExt, tool, tool_handler, tool_router};
use schemars::JsonSchema;
use serde::Deserialize;

use crate::config::GatewayState;

#[derive(Clone)]
pub struct McpServer {
    gateway: Arc<GatewayState>,
    tool_router: ToolRouter<Self>,
}

// --- Parameter types ---

#[derive(Deserialize, JsonSchema)]
pub struct CreateServiceParams {
    #[schemars(description = "Unique service ID, e.g. \"default\"")]
    pub id: String,
    #[schemars(description = "Human-readable service name")]
    pub name: String,
}

#[derive(Deserialize, JsonSchema)]
pub struct CreateProviderParams {
    #[schemars(description = "Provider name, e.g. \"openai-main\"")]
    pub name: String,
    #[schemars(description = "Provider type: \"openai\" or \"anthropic\"")]
    pub provider_type: String,
    #[schemars(description = "Model / endpoint ID, e.g. \"gpt-4o\"")]
    pub endpoint_id: String,
    #[schemars(description = "Base URL override (optional)")]
    pub base_url: Option<String>,
    #[schemars(description = "Upstream API key")]
    pub api_key: String,
    #[schemars(description = "JSON model mapping, e.g. {\"chat\":\"gpt-4o\"} (optional)")]
    pub model_mapping: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
pub struct BindProviderParams {
    #[schemars(description = "Service ID to bind to")]
    pub service_id: String,
    #[schemars(description = "Provider index (0-based, as returned by list_providers)")]
    pub provider_id: i64,
}

#[derive(Deserialize, JsonSchema)]
pub struct CreateApiKeyParams {
    #[schemars(description = "Gateway API key string, e.g. \"ugk_abc123\"")]
    pub key: String,
    #[schemars(description = "Service ID this key belongs to")]
    pub service_id: String,
    #[schemars(description = "Total request quota (optional)")]
    pub quota_limit: Option<i64>,
    #[schemars(description = "Max requests per second (optional)")]
    pub qps_limit: Option<f64>,
    #[schemars(description = "Max concurrent requests (optional)")]
    pub concurrency_limit: Option<i64>,
}

#[derive(Deserialize, JsonSchema)]
pub struct EmptyParams {}

// --- Tool implementations ---

#[tool_router]
impl McpServer {
    pub fn new(gateway: Arc<GatewayState>) -> Self {
        Self {
            gateway,
            tool_router: Self::tool_router(),
        }
    }

    #[tool(description = "List all configured gateway services (id, name, routing_strategy)")]
    async fn list_services(&self) -> String {
        let guard = self.gateway.inner.read().await;
        if guard.file.services.is_empty() {
            return "No services configured.".to_string();
        }
        let mut out = String::from("Services:\n");
        for s in &guard.file.services {
            out.push_str(&format!(
                "  - id={}, name={}, routing={}\n",
                s.id, s.name, s.routing_strategy
            ));
        }
        out
    }

    #[tool(description = "Create or update a gateway service")]
    async fn create_service(&self, params: Parameters<CreateServiceParams>) -> String {
        let p = params.0;
        self.gateway.create_service(&p.id, &p.name).await;
        if let Err(e) = self.gateway.persist_if_dirty().await {
            return format!("Service created in memory but failed to persist: {e}");
        }
        format!("Service created: id={}, name={}", p.id, p.name)
    }

    #[tool(
        description = "List all configured providers (index, name, type, endpoint_id, base_url)"
    )]
    async fn list_providers(&self) -> String {
        let providers = self.gateway.list_providers().await;
        if providers.is_empty() {
            return "No providers configured.".to_string();
        }
        let mut out = String::from("Providers:\n");
        for (idx, name, ptype, eid, url) in &providers {
            out.push_str(&format!(
                "  - [{}] name={}, type={}, endpoint={}, base_url={}\n",
                idx,
                name,
                ptype,
                eid.as_deref().unwrap_or("-"),
                url.as_deref().unwrap_or("(default)")
            ));
        }
        out
    }

    #[tool(description = "Create or update a provider with upstream LLM connection details")]
    async fn create_provider(&self, params: Parameters<CreateProviderParams>) -> String {
        let p = params.0;
        let idx = self
            .gateway
            .create_provider(
                &p.name,
                &p.provider_type,
                &p.endpoint_id,
                p.base_url.as_deref(),
                &p.api_key,
                p.model_mapping.as_deref(),
            )
            .await;
        if let Err(e) = self.gateway.persist_if_dirty().await {
            return format!("Provider created (id={idx}) but failed to persist: {e}");
        }
        format!("Provider created: name={}, provider_id={idx}", p.name)
    }

    #[tool(
        description = "Bind a provider to a service so the service can route requests through it"
    )]
    async fn bind_provider(&self, params: Parameters<BindProviderParams>) -> String {
        let p = params.0;
        match self
            .gateway
            .bind_provider_to_service(&p.service_id, p.provider_id)
            .await
        {
            Ok(()) => {
                if let Err(e) = self.gateway.persist_if_dirty().await {
                    return format!("Binding created but failed to persist: {e}");
                }
                format!(
                    "Provider {} bound to service {}",
                    p.provider_id, p.service_id
                )
            }
            Err(e) => format!("Failed to bind: {e}"),
        }
    }

    #[tool(description = "List all API keys (keys are masked for security)")]
    async fn list_api_keys(&self) -> String {
        let keys = self.gateway.list_api_keys().await;
        if keys.is_empty() {
            return "No API keys configured.".to_string();
        }
        let mut out = String::from("API Keys:\n");
        for k in &keys {
            let masked = if k.key.len() > 8 {
                format!("{}…{}", &k.key[..4], &k.key[k.key.len() - 4..])
            } else {
                "****".to_string()
            };
            out.push_str(&format!(
                "  - key={}, service={}, active={}, used={}, quota={}, qps={}, concurrency={}\n",
                masked,
                k.service_id,
                k.is_active,
                k.used_quota,
                k.quota_limit
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| "unlimited".into()),
                k.qps_limit
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| "unlimited".into()),
                k.concurrency_limit
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| "unlimited".into()),
            ));
        }
        out
    }

    #[tool(description = "Create or update a gateway API key with optional rate limits")]
    async fn create_api_key(&self, params: Parameters<CreateApiKeyParams>) -> String {
        let p = params.0;
        self.gateway
            .create_api_key(
                &p.key,
                &p.service_id,
                p.quota_limit,
                p.qps_limit,
                p.concurrency_limit,
            )
            .await;
        if let Err(e) = self.gateway.persist_if_dirty().await {
            return format!("API key created but failed to persist: {e}");
        }
        format!(
            "API key created: service={}, quota={}, qps={}, concurrency={}",
            p.service_id,
            p.quota_limit
                .map(|v| v.to_string())
                .unwrap_or_else(|| "unlimited".into()),
            p.qps_limit
                .map(|v| v.to_string())
                .unwrap_or_else(|| "unlimited".into()),
            p.concurrency_limit
                .map(|v| v.to_string())
                .unwrap_or_else(|| "unlimited".into()),
        )
    }

    #[tool(description = "Show the raw gateway config (services, providers, bindings, API keys)")]
    async fn show_config(&self) -> String {
        let guard = self.gateway.inner.read().await;
        match toml::to_string_pretty(&guard.file) {
            Ok(s) => s,
            Err(e) => format!("Failed to serialize config: {e}"),
        }
    }

    #[tool(description = "Get gateway request metrics snapshot")]
    async fn get_metrics(&self) -> String {
        let (total, openai, anthropic, embeddings) = self.gateway.metrics_snapshot().await;
        format!(
            "Metrics:\n  total_requests: {total}\n  openai_chat: {openai}\n  anthropic_messages: {anthropic}\n  embeddings: {embeddings}"
        )
    }

    #[tool(description = "Check if the UniGateway server process is running")]
    async fn server_status(&self, _params: Parameters<EmptyParams>) -> String {
        if let Some(pid) = crate::cli::process::is_running() {
            format!("UniGateway is running (pid: {})", pid)
        } else {
            "UniGateway is not running".to_string()
        }
    }

    #[tool(description = "Stop the background UniGateway server process")]
    async fn server_stop(&self, _params: Parameters<EmptyParams>) -> String {
        match crate::cli::process::stop_server() {
            Ok(()) => "UniGateway stopped successfully".to_string(),
            Err(e) => format!("Failed to stop UniGateway: {e}"),
        }
    }

    #[tool(description = "Start the UniGateway server process in background")]
    async fn server_start(&self, _params: Parameters<EmptyParams>) -> String {
        match crate::cli::process::daemonize() {
            Ok(()) => "UniGateway started successfully in background".to_string(),
            Err(e) => format!("Failed to start UniGateway: {e}"),
        }
    }
}

#[tool_handler]
impl ServerHandler for McpServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new(
                "unigateway",
                env!("CARGO_PKG_VERSION"),
            ))
            .with_instructions(
                "UniGateway MCP server – manage services, providers, API keys and view metrics for the LLM gateway.",
            )
    }
}

pub async fn run(config_path: &str) -> anyhow::Result<()> {
    let gateway = GatewayState::load(Path::new(config_path)).await?;
    let server = McpServer::new(gateway);
    let service = server.serve(stdio()).await?;
    service.waiting().await?;
    Ok(())
}
