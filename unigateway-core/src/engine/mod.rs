use std::sync::Arc;
use std::time::{Duration, SystemTime};

use tokio::sync::{Mutex, RwLock};

use crate::drivers::{DriverEndpointContext, DriverRegistry, ProviderDriver};
use crate::error::GatewayError;
use crate::feedback::RoutingFeedbackProvider;
use crate::hooks::{AttemptFinishedEvent, AttemptStartedEvent, GatewayHooks, RequestStartedEvent};
use crate::pool::{
    Endpoint, ExecutionTarget, PoolId, PoolSummary, ProviderKind, ProviderPool, RequestId,
};
use crate::response::{AttemptReport, RequestKind, RequestReport};
use crate::retry::RetryPolicy;
use crate::routing::ExecutionSnapshot;

mod aimd;
mod execution;
mod reporting;
pub use aimd::{AdaptiveConcurrency, AdaptiveConcurrencyConfig, AimdSnapshot};

#[cfg(test)]
mod tests;

use reporting::{
    build_failed_request_report, emit_attempt_finished_hook, emit_attempt_started_hook,
    emit_request_finished_hook,
};

struct EngineState {
    pools: RwLock<std::collections::HashMap<PoolId, ProviderPool>>,
    rr_counters: Mutex<std::collections::HashMap<String, usize>>,
    hooks: Option<Arc<dyn GatewayHooks>>,
    routing_feedback_provider: Option<Arc<dyn RoutingFeedbackProvider>>,
    driver_registry: Option<Arc<dyn DriverRegistry>>,
    default_retry_policy: RetryPolicy,
    default_timeout: Option<Duration>,
    aimd_config: Arc<AdaptiveConcurrencyConfig>,
    aimd_states: Mutex<std::collections::HashMap<String, Arc<AdaptiveConcurrency>>>,
}

/// The core asynchronous AI gateway engine.
/// Provides abstractions to mount `ProviderPool` objects and seamlessly multiplex
/// user inference request via failover limits, driver integrations, and retry policies.
pub struct UniGatewayEngine {
    inner: Arc<EngineState>,
}

struct FailedRequestContext {
    request_id: RequestId,
    pool_id: Option<PoolId>,
    endpoint_id: String,
    provider_kind: ProviderKind,
    started_at: SystemTime,
    metadata: std::collections::HashMap<String, String>,
}

/// Builder class to construct and configure a `UniGatewayEngine` cleanly.
#[derive(Default)]
pub struct UniGatewayEngineBuilder {
    /// Registered global hooks for the builder
    pub hooks: Option<Arc<dyn GatewayHooks>>,
    /// Optional provider for neutral endpoint routing feedback.
    pub routing_feedback_provider: Option<Arc<dyn RoutingFeedbackProvider>>,
    /// Pluggable driver registry defining supported transports and providers
    pub driver_registry: Option<Arc<dyn DriverRegistry>>,
    /// Global backoff/retry algorithm for any stateless execution
    pub default_retry_policy: RetryPolicy,
    /// Absolute cutoff duration per upstream request attempt
    pub default_timeout: Option<Duration>,
}

impl UniGatewayEngine {
    /// Creates a builder to customize the internals before startup.
    pub fn builder() -> UniGatewayEngineBuilder {
        UniGatewayEngineBuilder::default()
    }

    /// Adds or updates a provider pool dynamically in the active routing registry.
    pub async fn upsert_pool(&self, pool: ProviderPool) -> Result<(), GatewayError> {
        let mut guard = self.inner.pools.write().await;
        guard.insert(pool.pool_id.clone(), pool);
        Ok(())
    }

    /// Drops a pool and its related counters from routing. Next requests to this pool will yield `GatewayError::PoolNotFound`.
    pub async fn remove_pool(&self, pool_id: &str) -> Result<(), GatewayError> {
        let mut guard = self.inner.pools.write().await;
        guard.remove(pool_id);

        let mut rr_guard = self.inner.rr_counters.lock().await;
        rr_guard.retain(|bucket, _| !bucket.starts_with(&format!("pool:{pool_id}:")));

        Ok(())
    }

    /// Reads a copy of the pool definition by ID.
    pub async fn get_pool(&self, pool_id: &str) -> Option<ProviderPool> {
        let guard = self.inner.pools.read().await;
        guard.get(pool_id).cloned()
    }

    /// Returns an overview list of all registered provider pools.
    pub async fn list_pools(&self) -> Vec<PoolSummary> {
        let guard = self.inner.pools.read().await;
        let mut pools: Vec<PoolSummary> = guard
            .values()
            .map(|pool| PoolSummary {
                pool_id: pool.pool_id.clone(),
                endpoint_count: pool.endpoints.len(),
                load_balancing: pool.load_balancing.clone(),
                metadata: pool.metadata.clone(),
            })
            .collect();
        pools.sort_by(|left, right| left.pool_id.cmp(&right.pool_id));
        pools
    }

    /// Partially updates metadata for a specific endpoint within a pool.
    ///
    /// Merges the provided metadata keys into the endpoint's existing metadata.
    /// Returns an error if the pool or endpoint is not found.
    pub async fn update_endpoint_metadata(
        &self,
        pool_id: &str,
        endpoint_id: &str,
        metadata: std::collections::HashMap<String, String>,
    ) -> Result<(), GatewayError> {
        let mut guard = self.inner.pools.write().await;
        let pool = guard
            .get_mut(pool_id)
            .ok_or_else(|| GatewayError::PoolNotFound(pool_id.to_string()))?;
        let endpoint = pool
            .endpoints
            .iter_mut()
            .find(|ep| ep.endpoint_id == endpoint_id)
            .ok_or_else(|| {
                GatewayError::InvalidRequest(format!(
                    "endpoint {} not found in pool {}",
                    endpoint_id, pool_id
                ))
            })?;
        endpoint.metadata.extend(metadata);
        Ok(())
    }

    /// Updates the load-balancing strategy and/or retry policy for a pool
    /// without requiring a full pool upsert.
    pub async fn update_pool_config(
        &self,
        pool_id: &str,
        load_balancing: Option<crate::LoadBalancingStrategy>,
        retry_policy: Option<RetryPolicy>,
    ) -> Result<(), GatewayError> {
        let mut guard = self.inner.pools.write().await;
        let pool = guard
            .get_mut(pool_id)
            .ok_or_else(|| GatewayError::PoolNotFound(pool_id.to_string()))?;
        if let Some(lb) = load_balancing {
            pool.load_balancing = lb;
        }
        if let Some(rp) = retry_policy {
            pool.retry_policy = rp;
        }
        Ok(())
    }

    pub(crate) async fn execution_snapshot(
        &self,
        target: &ExecutionTarget,
    ) -> Result<ExecutionSnapshot, GatewayError> {
        let guard = self.inner.pools.read().await;
        crate::routing::build_execution_snapshot(
            &guard,
            target,
            &self.inner.default_retry_policy,
            self.inner.default_timeout,
            self.inner.routing_feedback_provider.as_ref(),
        )
    }

    #[cfg(test)]
    pub(crate) async fn select_endpoint_for_target(
        &self,
        target: &ExecutionTarget,
    ) -> Result<(ExecutionSnapshot, Endpoint), GatewayError> {
        let snapshot = self.execution_snapshot(target).await?;
        let mut rr_guard = self.inner.rr_counters.lock().await;
        let endpoint = snapshot.select_endpoint(&mut rr_guard)?;
        Ok((snapshot, endpoint))
    }

    async fn attempt_endpoints(
        &self,
        snapshot: &ExecutionSnapshot,
    ) -> Result<Vec<Endpoint>, GatewayError> {
        let mut rr_guard = self.inner.rr_counters.lock().await;
        snapshot.ordered_endpoints(&mut rr_guard, snapshot.retry_policy.max_attempts)
    }

    /// Retrieves the AIMD snapshot metrics for all known endpoints.
    pub async fn aimd_metrics(&self) -> std::collections::HashMap<String, AimdSnapshot> {
        let mut metrics = std::collections::HashMap::new();
        let guard = self.inner.aimd_states.lock().await;
        for (endpoint_id, aimd) in guard.iter() {
            metrics.insert(endpoint_id.clone(), aimd.snapshot());
        }
        metrics
    }

    async fn emit_attempt_started(&self, event: AttemptStartedEvent) {
        emit_attempt_started_hook(self.inner.hooks.clone(), event).await;
    }

    async fn emit_request_started(&self, event: RequestStartedEvent) {
        reporting::emit_request_started_hook(self.inner.hooks.clone(), event).await;
    }

    async fn emit_attempt_finished(&self, event: AttemptFinishedEvent) {
        emit_attempt_finished_hook(self.inner.hooks.clone(), event).await;
    }

    async fn emit_request_finished(&self, report: RequestReport) {
        emit_request_finished_hook(self.inner.hooks.clone(), report).await;
    }

    async fn finalize_request_failure(
        &self,
        context: FailedRequestContext,
        attempts: Vec<AttemptReport>,
        error: GatewayError,
        kind: RequestKind,
    ) -> GatewayError {
        if attempts.is_empty() {
            return error;
        }

        let report = build_failed_request_report(
            &context,
            attempts.clone(),
            SystemTime::now(),
            kind,
            None,
            Some(error.kind()),
        );
        self.emit_request_finished(report).await;

        GatewayError::AllAttemptsFailed {
            attempts,
            last_error: Box::new(error),
        }
    }

    fn driver_for_endpoint(
        &self,
        endpoint: &Endpoint,
    ) -> Result<Arc<dyn ProviderDriver>, GatewayError> {
        let Some(registry) = self.inner.driver_registry.as_ref() else {
            return Err(GatewayError::InvalidRequest(
                "driver registry not configured".to_string(),
            ));
        };

        registry.get(&endpoint.driver_id).ok_or_else(|| {
            GatewayError::InvalidRequest(format!("driver not found: {}", endpoint.driver_id))
        })
    }

    fn driver_context(
        &self,
        pool_id: Option<PoolId>,
        endpoint: Endpoint,
        snapshot_metadata: std::collections::HashMap<String, String>,
        request_metadata: std::collections::HashMap<String, String>,
    ) -> DriverEndpointContext {
        let mut metadata = snapshot_metadata;
        metadata.extend(endpoint.metadata.clone());
        if let Some(pool_id) = pool_id {
            metadata.entry("pool_id".to_string()).or_insert(pool_id);
        }
        metadata.extend(request_metadata);

        DriverEndpointContext {
            endpoint_id: endpoint.endpoint_id,
            provider_kind: endpoint.provider_kind,
            base_url: endpoint.base_url,
            api_key: endpoint.api_key,
            model_policy: endpoint.model_policy,
            metadata,
        }
    }

    async fn aimd_for_endpoint(&self, endpoint_id: &str) -> Arc<AdaptiveConcurrency> {
        let mut states = self.inner.aimd_states.lock().await;
        if let Some(aimd) = states.get(endpoint_id) {
            return aimd.clone();
        }
        let aimd = Arc::new(AdaptiveConcurrency::new(self.inner.aimd_config.clone()));
        states.insert(endpoint_id.to_string(), aimd.clone());
        aimd
    }
}

impl UniGatewayEngineBuilder {
    /// Attaches telemetry and lifecycle logging hooks to the pipeline constraint.
    pub fn with_hooks(mut self, hooks: Arc<dyn GatewayHooks>) -> Self {
        self.hooks = Some(hooks);
        self
    }

    /// Installs a neutral routing feedback provider that can suppress endpoints and provide a
    /// baseline candidate ordering before the pool's load-balancing strategy is applied.
    pub fn with_routing_feedback_provider(
        mut self,
        provider: Arc<dyn RoutingFeedbackProvider>,
    ) -> Self {
        self.routing_feedback_provider = Some(provider);
        self
    }

    /// Installs a specific driver repository for locating the concrete driver logic at runtime.
    pub fn with_driver_registry(mut self, registry: Arc<dyn DriverRegistry>) -> Self {
        self.driver_registry = Some(registry);
        self
    }

    /// Registers the built-in OpenAI-compatible and Anthropic drivers backed by the default
    /// `reqwest` HTTP transport. This is the zero-boilerplate starting point for most callers.
    ///
    /// Equivalent to manually creating an [`InMemoryDriverRegistry`], instantiating
    /// [`ReqwestHttpTransport`], calling [`builtin_drivers`], and passing the registry via
    /// [`with_driver_registry`].
    ///
    /// [`InMemoryDriverRegistry`]: crate::registry::InMemoryDriverRegistry
    /// [`ReqwestHttpTransport`]: crate::transport::ReqwestHttpTransport
    /// [`builtin_drivers`]: crate::protocol::builtin_drivers
    /// [`with_driver_registry`]: Self::with_driver_registry
    pub fn with_builtin_http_drivers(self) -> Self {
        use crate::registry::InMemoryDriverRegistry;
        use crate::transport::ReqwestHttpTransport;

        let transport = Arc::new(ReqwestHttpTransport::default());
        let registry = Arc::new(InMemoryDriverRegistry::new());
        for driver in crate::protocol::builtin_drivers(transport) {
            registry.register(driver);
        }
        self.with_driver_registry(registry)
    }

    /// Configures the default exponential or static retry behavior if a provider pool does not define its own.
    pub fn with_default_retry_policy(mut self, retry_policy: RetryPolicy) -> Self {
        self.default_retry_policy = retry_policy;
        self
    }

    /// Sets a hard global TCP connection and transfer timeout across all remote provider HTTP attempts.
    pub fn with_default_timeout(mut self, timeout: Duration) -> Self {
        self.default_timeout = Some(timeout);
        self
    }

    /// Finalizes the configuration and spins up the fully operational gateway engine.
    ///
    /// Can yield a `GatewayError::BuildError` if the configuration is semantically illegal.
    pub fn build(self) -> Result<UniGatewayEngine, crate::error::GatewayError> {
        if self.driver_registry.is_none() {
            return Err(crate::error::GatewayError::BuildError(
                "driver registry must be configured, consider calling .with_builtin_http_drivers()"
                    .to_string(),
            ));
        }

        Ok(UniGatewayEngine {
            inner: Arc::new(EngineState {
                pools: RwLock::new(std::collections::HashMap::new()),
                rr_counters: Mutex::new(std::collections::HashMap::new()),
                hooks: self.hooks,
                routing_feedback_provider: self.routing_feedback_provider,
                driver_registry: self.driver_registry,
                default_retry_policy: self.default_retry_policy,
                default_timeout: self.default_timeout,
                aimd_config: Arc::new(AdaptiveConcurrencyConfig::default()),
                aimd_states: Mutex::new(std::collections::HashMap::new()),
            }),
        })
    }
}
