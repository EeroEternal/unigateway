use std::sync::Arc;
use std::time::Duration;

use tokio::sync::{Mutex, RwLock};

use crate::drivers::{DriverEndpointContext, DriverRegistry, ProviderDriver};
use crate::error::GatewayError;
use crate::hooks::GatewayHooks;
use crate::pool::{Endpoint, ExecutionTarget, PoolId, PoolSummary, ProviderPool};
use crate::request::{ProxyChatRequest, ProxyEmbeddingsRequest, ProxyResponsesRequest};
use crate::response::{
    ChatResponseChunk, ChatResponseFinal, CompletedResponse, EmbeddingsResponse, ProxySession,
    ResponsesEvent, ResponsesFinal,
};
use crate::retry::RetryPolicy;
use crate::routing::ExecutionSnapshot;

struct EngineState {
    pools: RwLock<std::collections::HashMap<PoolId, ProviderPool>>,
    rr_counters: Mutex<std::collections::HashMap<String, usize>>,
    #[allow(dead_code)]
    hooks: Option<Arc<dyn GatewayHooks>>,
    #[allow(dead_code)]
    driver_registry: Option<Arc<dyn DriverRegistry>>,
    default_retry_policy: RetryPolicy,
    default_timeout: Option<Duration>,
}

pub struct UniGatewayEngine {
    inner: Arc<EngineState>,
}

#[derive(Default)]
pub struct UniGatewayEngineBuilder {
    pub hooks: Option<Arc<dyn GatewayHooks>>,
    pub driver_registry: Option<Arc<dyn DriverRegistry>>,
    pub default_retry_policy: RetryPolicy,
    pub default_timeout: Option<Duration>,
}

impl UniGatewayEngine {
    pub fn builder() -> UniGatewayEngineBuilder {
        UniGatewayEngineBuilder::default()
    }

    pub async fn upsert_pool(&self, pool: ProviderPool) -> Result<(), GatewayError> {
        let mut guard = self.inner.pools.write().await;
        guard.insert(pool.pool_id.clone(), pool);
        Ok(())
    }

    pub async fn remove_pool(&self, pool_id: &str) -> Result<(), GatewayError> {
        let mut guard = self.inner.pools.write().await;
        guard.remove(pool_id);

        let mut rr_guard = self.inner.rr_counters.lock().await;
        rr_guard.retain(|bucket, _| !bucket.starts_with(&format!("pool:{pool_id}:")));

        Ok(())
    }

    pub async fn get_pool(&self, pool_id: &str) -> Option<ProviderPool> {
        let guard = self.inner.pools.read().await;
        guard.get(pool_id).cloned()
    }

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

    pub async fn proxy_chat(
        &self,
        request: ProxyChatRequest,
        target: ExecutionTarget,
    ) -> Result<ProxySession<ChatResponseChunk, ChatResponseFinal>, GatewayError> {
        let (snapshot, endpoint) = self.select_endpoint_for_target(&target).await?;
        let driver = self.driver_for_endpoint(&endpoint)?;
        driver
            .execute_chat(
                self.driver_context(snapshot.pool_id, endpoint, snapshot.metadata),
                request,
            )
            .await
    }

    pub async fn proxy_responses(
        &self,
        request: ProxyResponsesRequest,
        target: ExecutionTarget,
    ) -> Result<ProxySession<ResponsesEvent, ResponsesFinal>, GatewayError> {
        let (snapshot, endpoint) = self.select_endpoint_for_target(&target).await?;
        let driver = self.driver_for_endpoint(&endpoint)?;
        driver
            .execute_responses(
                self.driver_context(snapshot.pool_id, endpoint, snapshot.metadata),
                request,
            )
            .await
    }

    pub async fn proxy_embeddings(
        &self,
        request: ProxyEmbeddingsRequest,
        target: ExecutionTarget,
    ) -> Result<CompletedResponse<EmbeddingsResponse>, GatewayError> {
        let (snapshot, endpoint) = self.select_endpoint_for_target(&target).await?;
        let driver = self.driver_for_endpoint(&endpoint)?;
        driver
            .execute_embeddings(
                self.driver_context(snapshot.pool_id, endpoint, snapshot.metadata),
                request,
            )
            .await
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
        )
    }

    pub(crate) async fn select_endpoint_for_target(
        &self,
        target: &ExecutionTarget,
    ) -> Result<(ExecutionSnapshot, Endpoint), GatewayError> {
        let snapshot = self.execution_snapshot(target).await?;
        let mut rr_guard = self.inner.rr_counters.lock().await;
        let endpoint = snapshot.select_endpoint(&mut rr_guard)?;
        Ok((snapshot, endpoint))
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
    ) -> DriverEndpointContext {
        let mut metadata = snapshot_metadata;
        metadata.extend(endpoint.metadata.clone());
        if let Some(pool_id) = pool_id {
            metadata.entry("pool_id".to_string()).or_insert(pool_id);
        }

        DriverEndpointContext {
            endpoint_id: endpoint.endpoint_id,
            provider_kind: endpoint.provider_kind,
            base_url: endpoint.base_url,
            api_key: endpoint.api_key,
            model_policy: endpoint.model_policy,
            metadata,
        }
    }
}

impl UniGatewayEngineBuilder {
    pub fn with_hooks(mut self, hooks: Arc<dyn GatewayHooks>) -> Self {
        self.hooks = Some(hooks);
        self
    }

    pub fn with_driver_registry(mut self, registry: Arc<dyn DriverRegistry>) -> Self {
        self.driver_registry = Some(registry);
        self
    }

    pub fn with_default_retry_policy(mut self, retry_policy: RetryPolicy) -> Self {
        self.default_retry_policy = retry_policy;
        self
    }

    pub fn with_default_timeout(mut self, timeout: Duration) -> Self {
        self.default_timeout = Some(timeout);
        self
    }

    pub fn build(self) -> UniGatewayEngine {
        UniGatewayEngine {
            inner: Arc::new(EngineState {
                pools: RwLock::new(std::collections::HashMap::new()),
                rr_counters: Mutex::new(std::collections::HashMap::new()),
                hooks: self.hooks,
                driver_registry: self.driver_registry,
                default_retry_policy: self.default_retry_policy,
                default_timeout: self.default_timeout,
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Arc;
    use std::time::SystemTime;

    use futures_util::future::BoxFuture;
    use serde_json::json;

    use crate::InMemoryDriverRegistry;
    use crate::drivers::{DriverEndpointContext, ProviderDriver};
    use crate::pool::{
        Endpoint, ExecutionPlan, ExecutionTarget, ProviderKind, ProviderPool, SecretString,
    };
    use crate::request::{ProxyChatRequest, ProxyEmbeddingsRequest, ProxyResponsesRequest};
    use crate::response::{
        ChatResponseChunk, ChatResponseFinal, CompletedResponse, EmbeddingsResponse, ProxySession,
        RequestReport, ResponsesEvent, ResponsesFinal,
    };
    use crate::retry::{BackoffPolicy, LoadBalancingStrategy, RetryCondition, RetryPolicy};

    use super::UniGatewayEngine;

    fn endpoint(endpoint_id: &str) -> Endpoint {
        Endpoint {
            endpoint_id: endpoint_id.to_string(),
            provider_kind: ProviderKind::OpenAiCompatible,
            driver_id: "openai-compatible".to_string(),
            base_url: format!("https://{endpoint_id}.example.com"),
            api_key: SecretString::new(format!("sk-{endpoint_id}")),
            model_policy: Default::default(),
            enabled: true,
            metadata: HashMap::new(),
        }
    }

    fn pool(
        pool_id: &str,
        strategy: LoadBalancingStrategy,
        endpoints: Vec<Endpoint>,
    ) -> ProviderPool {
        ProviderPool {
            pool_id: pool_id.to_string(),
            endpoints,
            load_balancing: strategy,
            retry_policy: RetryPolicy {
                max_attempts: 2,
                per_attempt_timeout: None,
                retry_on: vec![RetryCondition::HttpStatus(429)],
                backoff: BackoffPolicy::None,
                stop_after_stream_started: true,
            },
            metadata: HashMap::new(),
        }
    }

    #[tokio::test]
    async fn upsert_get_list_and_remove_pool() {
        let engine = UniGatewayEngine::builder().build();
        engine
            .upsert_pool(pool(
                "alpha",
                LoadBalancingStrategy::RoundRobin,
                vec![endpoint("a")],
            ))
            .await
            .expect("upsert");

        let stored = engine.get_pool("alpha").await.expect("stored pool");
        assert_eq!(stored.pool_id, "alpha");

        let listed = engine.list_pools().await;
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].pool_id, "alpha");

        engine.remove_pool("alpha").await.expect("remove pool");
        assert!(engine.get_pool("alpha").await.is_none());
    }

    #[tokio::test]
    async fn snapshot_is_stable_after_pool_update() {
        let engine = UniGatewayEngine::builder().build();
        engine
            .upsert_pool(pool(
                "alpha",
                LoadBalancingStrategy::RoundRobin,
                vec![endpoint("a")],
            ))
            .await
            .expect("upsert first pool");

        let snapshot = engine
            .execution_snapshot(&ExecutionTarget::Pool {
                pool_id: "alpha".to_string(),
            })
            .await
            .expect("first snapshot");

        engine
            .upsert_pool(pool(
                "alpha",
                LoadBalancingStrategy::RoundRobin,
                vec![endpoint("b")],
            ))
            .await
            .expect("upsert second pool");

        let next_snapshot = engine
            .execution_snapshot(&ExecutionTarget::Pool {
                pool_id: "alpha".to_string(),
            })
            .await
            .expect("second snapshot");

        assert_eq!(snapshot.pool_id.as_deref(), Some("alpha"));
        assert_eq!(snapshot.retry_policy.max_attempts, 2);
        assert_eq!(snapshot.endpoints[0].endpoint_id, "a");
        assert_eq!(next_snapshot.endpoints[0].endpoint_id, "b");
    }

    #[tokio::test]
    async fn round_robin_rotates_across_enabled_endpoints() {
        let engine = UniGatewayEngine::builder().build();
        engine
            .upsert_pool(pool(
                "alpha",
                LoadBalancingStrategy::RoundRobin,
                vec![endpoint("a"), endpoint("b")],
            ))
            .await
            .expect("upsert pool");

        let (_, first) = engine
            .select_endpoint_for_target(&ExecutionTarget::Pool {
                pool_id: "alpha".to_string(),
            })
            .await
            .expect("first selection");
        let (_, second) = engine
            .select_endpoint_for_target(&ExecutionTarget::Pool {
                pool_id: "alpha".to_string(),
            })
            .await
            .expect("second selection");

        assert_eq!(first.endpoint_id, "a");
        assert_eq!(second.endpoint_id, "b");
    }

    #[tokio::test]
    async fn execution_plan_uses_candidate_subset() {
        let engine = UniGatewayEngine::builder().build();
        engine
            .upsert_pool(pool(
                "alpha",
                LoadBalancingStrategy::RoundRobin,
                vec![endpoint("a"), endpoint("b"), endpoint("c")],
            ))
            .await
            .expect("upsert pool");

        let snapshot = engine
            .execution_snapshot(&ExecutionTarget::Plan(ExecutionPlan {
                pool_id: Some("alpha".to_string()),
                candidates: vec![
                    crate::pool::EndpointRef {
                        endpoint_id: "b".to_string(),
                    },
                    crate::pool::EndpointRef {
                        endpoint_id: "c".to_string(),
                    },
                ],
                load_balancing_override: Some(LoadBalancingStrategy::Random),
                retry_policy_override: None,
                metadata: HashMap::new(),
            }))
            .await
            .expect("plan snapshot");

        assert_eq!(snapshot.pool_id.as_deref(), Some("alpha"));
        assert!(snapshot.metadata.is_empty());
        assert_eq!(snapshot.endpoints.len(), 2);
        assert_eq!(snapshot.load_balancing, LoadBalancingStrategy::Random);
        assert!(
            snapshot
                .endpoints
                .iter()
                .all(|item| item.endpoint_id == "b" || item.endpoint_id == "c")
        );
    }

    struct TestDriver;

    impl ProviderDriver for TestDriver {
        fn driver_id(&self) -> &str {
            "openai-compatible"
        }

        fn provider_kind(&self) -> ProviderKind {
            ProviderKind::OpenAiCompatible
        }

        fn execute_chat(
            &self,
            endpoint: DriverEndpointContext,
            request: ProxyChatRequest,
        ) -> BoxFuture<
            'static,
            Result<ProxySession<ChatResponseChunk, ChatResponseFinal>, crate::error::GatewayError>,
        > {
            Box::pin(async move {
                Ok(ProxySession::Completed(CompletedResponse {
                    response: ChatResponseFinal {
                        model: Some(request.model),
                        output_text: Some(endpoint.endpoint_id.clone()),
                        raw: json!({"endpoint_id": endpoint.endpoint_id}),
                    },
                    report: RequestReport {
                        request_id: "req-1".to_string(),
                        pool_id: endpoint.metadata.get("pool_id").cloned(),
                        selected_endpoint_id: endpoint.endpoint_id,
                        selected_provider: endpoint.provider_kind,
                        attempts: Vec::new(),
                        usage: None,
                        latency_ms: 1,
                        started_at: SystemTime::UNIX_EPOCH,
                        finished_at: SystemTime::UNIX_EPOCH,
                        metadata: endpoint.metadata,
                    },
                }))
            })
        }

        fn execute_responses(
            &self,
            endpoint: DriverEndpointContext,
            _request: ProxyResponsesRequest,
        ) -> BoxFuture<
            'static,
            Result<ProxySession<ResponsesEvent, ResponsesFinal>, crate::error::GatewayError>,
        > {
            Box::pin(async move {
                Ok(ProxySession::Completed(CompletedResponse {
                    response: ResponsesFinal {
                        output_text: Some(endpoint.endpoint_id.clone()),
                        raw: json!({"endpoint_id": endpoint.endpoint_id}),
                    },
                    report: RequestReport {
                        request_id: "req-2".to_string(),
                        pool_id: endpoint.metadata.get("pool_id").cloned(),
                        selected_endpoint_id: endpoint.endpoint_id,
                        selected_provider: endpoint.provider_kind,
                        attempts: Vec::new(),
                        usage: None,
                        latency_ms: 1,
                        started_at: SystemTime::UNIX_EPOCH,
                        finished_at: SystemTime::UNIX_EPOCH,
                        metadata: endpoint.metadata,
                    },
                }))
            })
        }

        fn execute_embeddings(
            &self,
            endpoint: DriverEndpointContext,
            _request: ProxyEmbeddingsRequest,
        ) -> BoxFuture<
            'static,
            Result<CompletedResponse<EmbeddingsResponse>, crate::error::GatewayError>,
        > {
            Box::pin(async move {
                Ok(CompletedResponse {
                    response: EmbeddingsResponse {
                        raw: json!({"endpoint_id": endpoint.endpoint_id}),
                    },
                    report: RequestReport {
                        request_id: "req-3".to_string(),
                        pool_id: endpoint.metadata.get("pool_id").cloned(),
                        selected_endpoint_id: endpoint.endpoint_id,
                        selected_provider: endpoint.provider_kind,
                        attempts: Vec::new(),
                        usage: None,
                        latency_ms: 1,
                        started_at: SystemTime::UNIX_EPOCH,
                        finished_at: SystemTime::UNIX_EPOCH,
                        metadata: endpoint.metadata,
                    },
                })
            })
        }
    }

    #[tokio::test]
    async fn proxy_chat_delegates_to_registered_driver() {
        let registry = Arc::new(InMemoryDriverRegistry::new());
        registry.register(Arc::new(TestDriver));

        let engine = UniGatewayEngine::builder()
            .with_driver_registry(registry)
            .build();
        engine
            .upsert_pool(pool(
                "alpha",
                LoadBalancingStrategy::RoundRobin,
                vec![endpoint("a")],
            ))
            .await
            .expect("upsert pool");

        let session = engine
            .proxy_chat(
                ProxyChatRequest {
                    model: "gpt-4o-mini".to_string(),
                    messages: Vec::new(),
                    temperature: None,
                    top_p: None,
                    max_tokens: None,
                    stream: false,
                    metadata: HashMap::new(),
                },
                ExecutionTarget::Pool {
                    pool_id: "alpha".to_string(),
                },
            )
            .await
            .expect("proxy chat");

        match session {
            ProxySession::Completed(result) => {
                assert_eq!(result.report.selected_endpoint_id, "a");
                assert_eq!(result.report.pool_id.as_deref(), Some("alpha"));
                assert_eq!(result.response.output_text.as_deref(), Some("a"));
            }
            ProxySession::Streaming(_) => panic!("expected completed response"),
        }
    }

    #[tokio::test]
    async fn proxy_chat_fails_when_driver_missing() {
        let engine = UniGatewayEngine::builder().build();
        engine
            .upsert_pool(pool(
                "alpha",
                LoadBalancingStrategy::RoundRobin,
                vec![endpoint("a")],
            ))
            .await
            .expect("upsert pool");

        let error = match engine
            .proxy_chat(
                ProxyChatRequest {
                    model: "gpt-4o-mini".to_string(),
                    messages: Vec::new(),
                    temperature: None,
                    top_p: None,
                    max_tokens: None,
                    stream: false,
                    metadata: HashMap::new(),
                },
                ExecutionTarget::Pool {
                    pool_id: "alpha".to_string(),
                },
            )
            .await
        {
            Ok(_) => panic!("missing driver registry should fail"),
            Err(error) => error,
        };

        assert!(error.to_string().contains("driver registry not configured"));
    }
}
