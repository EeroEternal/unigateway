//! Config file + in-memory state: load from TOML, mutate in memory, persist back to file.

mod admin;
pub(crate) mod core_sync;
mod schema;
mod select;
mod store;

use std::collections::HashMap;
use std::time::Instant;

use tokio::sync::{Mutex, RwLock, mpsc, Notify};
use std::sync::Arc;

use self::schema::default_round_robin;
pub use self::schema::{
    ApiKeyEntry, BindingEntry, GatewayApiKey, GatewayConfigFile, ModeKey, ModeProvider, ModeView,
    ProviderEntry, ProviderModelOptions, ServiceEntry, ServiceProvider, build_mode_views,
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
    pub config_path: std::path::PathBuf,
    pub inner: RwLock<GatewayConfig>,
    pub api_key_runtime: Mutex<HashMap<String, RuntimeRateState>>,
    pub service_rr: Mutex<HashMap<String, usize>>,
    pub core_sync_notifier: Mutex<Option<mpsc::UnboundedSender<()>>>,
}

pub static QPS_SLEEPERS_COUNT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
pub const MAX_QPS_SLEEPERS: usize = 2000;

#[derive(Debug, Clone)]
pub struct RuntimeRateState {
    pub last_update: Instant,
    pub tokens: f64,
    pub in_flight: u64,
    pub in_queue: u64,
    pub notify: Arc<Notify>,
}
