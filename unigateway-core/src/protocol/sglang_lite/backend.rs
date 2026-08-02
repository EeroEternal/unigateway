use std::collections::HashMap;
use std::time::Duration;

use tokio::process::{Child, Command};
use tokio::time::{Instant, sleep, timeout};

use crate::error::GatewayError;
use crate::transport::{HttpMethod, HttpTransport, TransportRequest};

const DEFAULT_BACKEND_MODE: &str = "http";
const DEFAULT_SUBPROCESS_STARTUP_TIMEOUT_MS: u64 = 30_000;
const DEFAULT_SUBPROCESS_HEALTH_PATH: &str = "health";

/// Metadata key selecting the sglang-lite communication backend.
pub const BACKEND_MODE_KEY: &str = "unigateway.sglang_lite.backend_mode";
/// Metadata key for the subprocess executable command.
pub const SUBPROCESS_COMMAND_KEY: &str = "unigateway.sglang_lite.subprocess.command";
/// Metadata key for the subprocess command arguments (space-separated).
pub const SUBPROCESS_ARGS_KEY: &str = "unigateway.sglang_lite.subprocess.args";
/// Metadata key for the subprocess startup timeout in milliseconds.
pub const SUBPROCESS_STARTUP_TIMEOUT_MS_KEY: &str =
    "unigateway.sglang_lite.subprocess.startup_timeout_ms";
/// Metadata key for the subprocess health check path.
pub const SUBPROCESS_HEALTH_PATH_KEY: &str = "unigateway.sglang_lite.subprocess.health_path";

/// Communication backend used by the sglang-lite driver.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SglangLiteBackend {
    /// Direct HTTP connection to an already-running sglang-lite server.
    Http,
    /// Spawn a local sglang-lite process and connect over HTTP.
    Subprocess(SglangLiteSubprocessConfig),
    /// gRPC backend (future / P2).
    ///
    /// The contract is defined in the sglang-lite repository:
    /// - proto/sglang_lite.proto (package `sglang_lite`, service `SglangLiteService`)
    /// - docs/sglang-lite-grpc-spec.md
    ///
    /// Key points from the confirmed spec:
    /// - Uses standard `grpc.health.v1.Health` (via tonic-health) for readiness checks.
    /// - `ChatCompletions` (unary) + `ChatCompletionsStream` (server streaming)
    /// - `Embeddings`, `ListModels`
    /// - `Usage` message includes `cache_hit_tokens`
    /// - Default port 50051, no TLS/auth for local use.
    /// - Error mapping via standard gRPC status codes.
    ///
    /// Currently returns `not_implemented`. When implementing, map
    /// `ProxyChatRequest` <-> protobuf messages and use a gRPC transport.
    Grpc,
}

impl SglangLiteBackend {
    /// Resolve the backend from endpoint metadata.
    pub fn from_metadata(
        metadata: &HashMap<String, String>,
        base_url: &str,
    ) -> Result<Self, GatewayError> {
        let mode = metadata
            .get(BACKEND_MODE_KEY)
            .map(String::as_str)
            .unwrap_or(DEFAULT_BACKEND_MODE);

        match mode {
            "subprocess" => Ok(Self::Subprocess(SglangLiteSubprocessConfig::from_metadata(
                metadata, base_url,
            )?)),
            "grpc" => Ok(Self::Grpc),
            _ => Ok(Self::Http),
        }
    }
}

/// Configuration for the subprocess backend.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SglangLiteSubprocessConfig {
    /// Executable command to spawn.
    pub command: String,
    /// Arguments passed to the command (already split).
    pub args: Vec<String>,
    /// How long to wait for the subprocess HTTP server to become ready.
    pub startup_timeout_ms: u64,
    /// HTTP path used for readiness checks (relative to `base_url`).
    pub health_path: String,
    /// Base URL the subprocess is expected to listen on.
    pub base_url: String,
}

impl SglangLiteSubprocessConfig {
    pub(crate) fn from_metadata(
        metadata: &HashMap<String, String>,
        base_url: &str,
    ) -> Result<Self, GatewayError> {
        let command = metadata
            .get(SUBPROCESS_COMMAND_KEY)
            .cloned()
            .ok_or_else(|| {
                GatewayError::InvalidRequest(
                    "sglang-lite subprocess backend requires 'subprocess.command'".to_string(),
                )
            })?;

        let args = metadata
            .get(SUBPROCESS_ARGS_KEY)
            .map(|raw| raw.split_whitespace().map(String::from).collect())
            .unwrap_or_default();

        let startup_timeout_ms = metadata
            .get(SUBPROCESS_STARTUP_TIMEOUT_MS_KEY)
            .and_then(|value| value.parse().ok())
            .unwrap_or(DEFAULT_SUBPROCESS_STARTUP_TIMEOUT_MS);

        let health_path = metadata
            .get(SUBPROCESS_HEALTH_PATH_KEY)
            .cloned()
            .unwrap_or_else(|| DEFAULT_SUBPROCESS_HEALTH_PATH.to_string());

        Ok(Self {
            command,
            args,
            startup_timeout_ms,
            health_path,
            base_url: base_url.trim_end_matches('/').to_string(),
        })
    }
}

/// Manages the lifecycle of a sglang-lite subprocess.
#[derive(Debug)]
pub struct SglangLiteSubprocess {
    config: SglangLiteSubprocessConfig,
}

impl SglangLiteSubprocess {
    /// Create a new subprocess manager without starting it yet.
    pub fn new(config: SglangLiteSubprocessConfig) -> Self {
        Self { config }
    }

    /// Spawn the subprocess and wait for its HTTP health endpoint to become ready.
    pub async fn start(&self, transport: &dyn HttpTransport) -> Result<Child, GatewayError> {
        let mut child = Command::new(&self.config.command)
            .args(&self.config.args)
            .spawn()
            .map_err(|error| GatewayError::Transport {
                message: format!("failed to spawn sglang-lite subprocess: {error}"),
                endpoint_id: None,
            })?;

        let deadline = Instant::now() + Duration::from_millis(self.config.startup_timeout_ms);

        let result = timeout(
            Duration::from_millis(self.config.startup_timeout_ms),
            self.wait_for_ready(transport, deadline),
        )
        .await;

        match result {
            Ok(Ok(())) => Ok(child),
            Ok(Err(error)) => {
                let _ = child.kill().await;
                Err(error)
            }
            Err(_) => {
                let _ = child.kill().await;
                Err(GatewayError::Transport {
                    message: format!(
                        "sglang-lite subprocess did not become ready within {} ms",
                        self.config.startup_timeout_ms
                    ),
                    endpoint_id: None,
                })
            }
        }
    }

    async fn wait_for_ready(
        &self,
        transport: &dyn HttpTransport,
        deadline: Instant,
    ) -> Result<(), GatewayError> {
        let url = format!("{}/{}", self.config.base_url, self.config.health_path);
        let request = TransportRequest {
            endpoint_id: None,
            method: HttpMethod::Get,
            url,
            headers: HashMap::new(),
            body: None,
            timeout: Some(Duration::from_millis(500)),
        };

        loop {
            match transport.send(request.clone()).await {
                Ok(response) if (200..300).contains(&response.status) => return Ok(()),
                _ => {
                    if Instant::now() >= deadline {
                        return Err(GatewayError::Transport {
                            message: "sglang-lite subprocess health check failed".to_string(),
                            endpoint_id: None,
                        });
                    }
                    sleep(Duration::from_millis(100)).await;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backend_from_metadata_defaults_to_http() {
        let metadata = HashMap::new();
        let backend = SglangLiteBackend::from_metadata(&metadata, "http://localhost:8000").unwrap();
        assert_eq!(backend, SglangLiteBackend::Http);
    }

    #[test]
    fn backend_from_metadata_parses_subprocess_config() {
        let metadata = HashMap::from([
            (BACKEND_MODE_KEY.to_string(), "subprocess".to_string()),
            (SUBPROCESS_COMMAND_KEY.to_string(), "python".to_string()),
            (
                SUBPROCESS_ARGS_KEY.to_string(),
                "-m sglang_lite.server --port 9000".to_string(),
            ),
            (
                SUBPROCESS_STARTUP_TIMEOUT_MS_KEY.to_string(),
                "5000".to_string(),
            ),
            (
                SUBPROCESS_HEALTH_PATH_KEY.to_string(),
                "healthz".to_string(),
            ),
        ]);

        let backend = SglangLiteBackend::from_metadata(&metadata, "http://localhost:9000").unwrap();
        let SglangLiteBackend::Subprocess(config) = backend else {
            panic!("expected subprocess backend");
        };

        assert_eq!(config.command, "python");
        assert_eq!(
            config.args,
            vec!["-m", "sglang_lite.server", "--port", "9000"]
        );
        assert_eq!(config.startup_timeout_ms, 5000);
        assert_eq!(config.health_path, "healthz");
        assert_eq!(config.base_url, "http://localhost:9000");
    }

    #[test]
    fn backend_from_metadata_subprocess_requires_command() {
        let metadata = HashMap::from([(BACKEND_MODE_KEY.to_string(), "subprocess".to_string())]);

        let result = SglangLiteBackend::from_metadata(&metadata, "http://localhost:8000");
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("subprocess.command")
        );
    }
}
