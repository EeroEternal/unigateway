//! Config file + in-memory state: load from TOML, mutate in memory, persist back to file.

mod admin;
pub mod core_sync;
pub mod routing;
mod runtime;
mod schema;
mod select;
mod store;

use std::collections::HashMap;
use std::time::Instant;

use std::sync::Arc;
use tokio::sync::{Mutex, MutexGuard, Notify, RwLock, RwLockReadGuard, RwLockWriteGuard, mpsc};

pub use self::schema::ServiceProvider;
use self::schema::default_round_robin;
pub use self::schema::{
    ApiKeyEntry, BindingEntry, GatewayApiKey, GatewayConfigFile, ModeKey, ModeProvider, ModeView,
    ProviderEntry, ProviderModelOptions, ServiceEntry, build_mode_views,
};

pub const MAX_QUEUE_PER_KEY: u64 = 100;
pub const QPS_SHAPING_TIMEOUT: tokio::time::Duration = tokio::time::Duration::from_millis(500); // 500ms max for QPS bursting sleep
pub const CONCURRENCY_QUEUE_TIMEOUT: tokio::time::Duration = tokio::time::Duration::from_secs(30); // 30s max for waiting on concurrency capacity

#[derive(Debug, Clone)]
pub struct RequestStats {
    pub total: u64,
    pub openai_total: u64,
    pub anthropic_total: u64,
    pub embeddings_total: u64,
}

#[derive(Debug)]
pub struct GatewayConfig {
    pub file: GatewayConfigFile,
    pub request_stats: RequestStats,
    pub dirty: bool,
}

impl Default for GatewayConfig {
    fn default() -> Self {
        Self {
            file: GatewayConfigFile::default(),
            request_stats: RequestStats {
                total: 0,
                openai_total: 0,
                anthropic_total: 0,
                embeddings_total: 0,
            },
            dirty: false,
        }
    }
}

pub struct GatewayState {
    config_store: ConfigStore,
    runtime_rate_limiter: RuntimeRateLimiter,
}

struct ConfigStore {
    path: std::path::PathBuf,
    inner: RwLock<GatewayConfig>,
    core_sync_notifier: Mutex<Option<mpsc::UnboundedSender<()>>>,
}

struct RuntimeRateLimiter {
    api_keys: Mutex<HashMap<String, RuntimeRateState>>,
}

pub static QPS_SLEEPERS_COUNT: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);
pub const MAX_QPS_SLEEPERS: usize = 2000;

#[derive(Debug, Clone)]
pub struct RuntimeRateSnapshot {
    pub tokens: f64,
    pub in_flight: u64,
    pub in_queue: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeLimitError {
    QpsWaitTooLong,
    TooManyQpsSleepers,
    QueueDepthExceeded,
    QueueTimeout,
    StateLost,
}

#[derive(Debug, Clone)]
pub struct RuntimeRateState {
    pub last_update: Instant,
    pub tokens: f64,
    pub in_flight: u64,
    pub in_queue: u64,
    pub notify: Arc<Notify>,
}

impl GatewayState {
    pub(crate) async fn read_config(&self) -> RwLockReadGuard<'_, GatewayConfig> {
        self.config_store.inner.read().await
    }

    pub(crate) async fn write_config(&self) -> RwLockWriteGuard<'_, GatewayConfig> {
        self.config_store.inner.write().await
    }

    pub(crate) async fn runtime_state(&self) -> MutexGuard<'_, HashMap<String, RuntimeRateState>> {
        self.runtime_rate_limiter.api_keys.lock().await
    }
}
