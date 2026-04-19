use std::future::Future;
use std::pin::Pin;

use unigateway_core::{ProviderPool, UniGatewayEngine};

pub use crate::error::{PoolLookupError, PoolLookupResult};

pub type HostFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Explicit outcome for host-side pool lookup.
///
/// This replaces `Option<ProviderPool>` so embedders can distinguish lookup success from
/// lookup failure without overloading `None` semantics.
///
/// Future variants may represent states such as disabled, not configured, or temporarily
/// unavailable pools, so external consumers should keep a fallback arm when matching.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub enum PoolLookupOutcome {
    /// A concrete provider pool was resolved and can be dispatched immediately.
    Found(ProviderPool),
    /// No provider pool is currently available for the requested service or env fallback.
    NotFound,
}

impl PoolLookupOutcome {
    /// Construct a successful pool lookup outcome.
    pub fn found(pool: ProviderPool) -> Self {
        Self::Found(pool)
    }

    /// Construct a missing-pool outcome.
    pub fn not_found() -> Self {
        Self::NotFound
    }
}

#[derive(Clone, Copy)]
pub struct HostContext<'a> {
    engine: &'a UniGatewayEngine,
    pool_host: &'a dyn PoolHost,
}

/// Provides per-request access to the pool that should serve a given service.
///
/// # Contract for implementors
///
/// The pool returned here **must already be registered** in the engine via
/// [`UniGatewayEngine::upsert_pool`] before this method is called. The host
/// execution helpers re-exported from [`crate::core`] build an
/// [`ExecutionTarget::Pool`][unigateway_core::ExecutionTarget] from the returned pool id and
/// then ask the engine to resolve it - if the pool has not been upserted the engine
/// will return [`GatewayError::PoolNotFound`][unigateway_core::GatewayError::PoolNotFound].
///
/// The recommended lifecycle for embedders is:
///
/// 1. **Startup sync** - call `engine.upsert_pool(pool)` for every pool fetched from
///    your datastore.
/// 2. **Hot updates** - whenever a pool changes, call `engine.upsert_pool(pool)` or
///    `engine.remove_pool(pool_id)`.
/// 3. **Per-request** - implement this method as a fast in-memory look-up that returns
///    `engine.get_pool(service_id)` (or equivalent). Do **not** query an external
///    datastore on every request.
pub trait PoolHost: Send + Sync {
    fn pool_for_service<'a>(
        &'a self,
        service_id: &'a str,
    ) -> HostFuture<'a, PoolLookupResult<PoolLookupOutcome>>;
}

impl<'a> HostContext<'a> {
    pub fn from_parts(engine: &'a UniGatewayEngine, pool_host: &'a dyn PoolHost) -> Self {
        Self { engine, pool_host }
    }

    pub fn core_engine(&self) -> &UniGatewayEngine {
        self.engine
    }

    pub async fn pool_for_service(&self, service_id: &str) -> PoolLookupResult<PoolLookupOutcome> {
        self.pool_host.pool_for_service(service_id).await
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use unigateway_core::{LoadBalancingStrategy, ProviderPool, RetryPolicy, UniGatewayEngine};

    use super::{HostContext, HostFuture, PoolHost, PoolLookupOutcome, PoolLookupResult};

    struct MockPoolHost;

    impl PoolHost for MockPoolHost {
        fn pool_for_service<'a>(
            &'a self,
            service_id: &'a str,
        ) -> HostFuture<'a, PoolLookupResult<PoolLookupOutcome>> {
            Box::pin(async move {
                Ok(PoolLookupOutcome::found(ProviderPool {
                    pool_id: format!("pool:{service_id}"),
                    endpoints: Vec::new(),
                    load_balancing: LoadBalancingStrategy::RoundRobin,
                    retry_policy: RetryPolicy::default(),
                    metadata: HashMap::new(),
                }))
            })
        }
    }

    #[tokio::test]
    async fn host_context_provides_engine_and_pool_access() {
        let engine = UniGatewayEngine::builder()
            .with_builtin_http_drivers()
            .build()
            .unwrap();
        let pool_host = MockPoolHost;

        let context = HostContext::from_parts(&engine, &pool_host);

        assert!(std::ptr::eq(context.core_engine(), &engine));

        let pool = context
            .pool_for_service("svc-main")
            .await
            .expect("pool lookup succeeds");
        let PoolLookupOutcome::Found(pool) = pool else {
            panic!("pool exists");
        };
        assert_eq!(pool.pool_id, "pool:svc-main");
    }
}
