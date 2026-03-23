//! Config file + in-memory state: load from TOML, mutate in memory, persist back to file.

mod admin;
mod schema;
mod select;
mod store;

use std::collections::HashMap;
use std::time::Instant;

use tokio::sync::{Mutex, RwLock};

use self::schema::default_round_robin;
pub use self::schema::{
    ApiKeyEntry, BindingEntry, GatewayApiKey, GatewayConfigFile, ModeKey, ModeProvider, ModeView,
    ProviderEntry, ProviderModelOptions, ServiceEntry, ServiceProvider, build_mode_views,
};

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
}

#[derive(Debug, Clone)]
pub struct RuntimeRateState {
    pub window_started_at: Instant,
    pub request_count: u64,
    pub in_flight: u64,
}
