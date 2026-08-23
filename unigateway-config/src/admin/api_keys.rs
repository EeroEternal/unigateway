//! Admin API-key lifecycle: listing, creation, read-only authorization,
//! and service rebinding.

use anyhow::Result;

use crate::{ApiKeyEntry, AuthError, GatewayApiKey, GatewayState};

impl GatewayState {
    pub async fn authorize_readonly(&self, raw_key: &str) -> Result<GatewayApiKey, AuthError> {
        let key = self
            .find_gateway_api_key(raw_key)
            .await
            .ok_or(AuthError::InvalidKey)?;
        if key.is_active != 1 {
            return Err(AuthError::InactiveKey);
        }
        Ok(key)
    }

    pub async fn list_api_keys(&self) -> Vec<ApiKeyEntry> {
        let guard = self.read_config().await;
        guard.file.api_keys.clone()
    }

    pub async fn create_api_key(
        &self,
        key: &str,
        service_id: &str,
        quota_limit: Option<i64>,
        qps_limit: Option<f64>,
        concurrency_limit: Option<i64>,
    ) {
        let mut guard = self.write_config().await;
        let used = guard
            .file
            .api_keys
            .iter()
            .find(|a| a.key == key)
            .map(|a| a.used_quota)
            .unwrap_or(0);
        let entry = ApiKeyEntry {
            key: key.to_string(),
            service_id: service_id.to_string(),
            quota_limit,
            used_quota: used,
            is_active: true,
            qps_limit,
            concurrency_limit,
        };
        if let Some(a) = guard.file.api_keys.iter_mut().find(|a| a.key == key) {
            *a = entry;
        } else {
            guard.file.api_keys.push(entry);
        }
        guard.dirty = true;
    }

    pub async fn rebind_api_key_service(&self, key: &str, service_id: &str) -> Result<()> {
        let mut guard = self.write_config().await;
        if !guard
            .file
            .services
            .iter()
            .any(|service| service.id == service_id)
        {
            anyhow::bail!("service '{}' not found", service_id);
        }

        let Some(api_key) = guard
            .file
            .api_keys
            .iter_mut()
            .find(|api_key| api_key.key == key)
        else {
            anyhow::bail!("api key '{}' not found", key);
        };

        if api_key.service_id != service_id {
            api_key.service_id = service_id.to_string();
            guard.dirty = true;
        }
        Ok(())
    }
}
