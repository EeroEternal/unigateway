use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, Result};
use tokio::sync::mpsc;

use super::{GatewayConfig, GatewayState, RequestStats};
use crate::config::GatewayConfigFile;

impl GatewayState {
    pub async fn load(config_path: &Path) -> Result<Arc<Self>> {
        let path = config_path.to_path_buf();
        let file = if path.exists() {
            let s = tokio::fs::read_to_string(&path)
                .await
                .with_context(|| format!("read config: {}", path.display()))?;
            toml::from_str::<GatewayConfigFile>(&s).context("parse config TOML")?
        } else {
            GatewayConfigFile::default()
        };
        let config = GatewayConfig {
            file,
            request_stats: RequestStats {
                total: 0,
                openai_total: 0,
                anthropic_total: 0,
                embeddings_total: 0,
            },
            dirty: false,
        };
        Ok(Arc::new(Self {
            config_path: path,
            inner: tokio::sync::RwLock::new(config),
            api_key_runtime: tokio::sync::Mutex::new(std::collections::HashMap::new()),
            service_rr: tokio::sync::Mutex::new(std::collections::HashMap::new()),
            core_sync_notifier: tokio::sync::Mutex::new(None),
        }))
    }

    pub async fn set_core_sync_notifier(&self, notifier: mpsc::UnboundedSender<()>) {
        *self.core_sync_notifier.lock().await = Some(notifier);
    }

    pub async fn request_core_sync(&self) {
        if let Some(notifier) = self.core_sync_notifier.lock().await.as_ref() {
            let _ = notifier.send(());
        }
    }

    pub async fn persist(&self) -> Result<()> {
        let to_write = {
            let guard = self.inner.read().await;
            if !guard.dirty {
                return Ok(());
            }
            guard.file.clone()
        };
        let s = toml::to_string_pretty(&to_write).context("serialize config")?;
        if let Some(parent) = self.config_path.parent() {
            tokio::fs::create_dir_all(parent).await.ok();
        }
        let tmp = self.config_path.with_extension("tmp");
        tokio::fs::write(&tmp, s)
            .await
            .with_context(|| format!("write config: {}", tmp.display()))?;
        tokio::fs::rename(&tmp, &self.config_path)
            .await
            .with_context(|| format!("rename config: {}", self.config_path.display()))?;
        self.inner.write().await.dirty = false;
        Ok(())
    }

    pub async fn persist_if_dirty(&self) -> Result<()> {
        if self.inner.read().await.dirty {
            self.persist().await
        } else {
            Ok(())
        }
    }
}
