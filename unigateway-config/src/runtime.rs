use std::time::{Duration, Instant};

use super::{
    CONCURRENCY_QUEUE_TIMEOUT, GatewayApiKey, GatewayState, MAX_QPS_SLEEPERS, MAX_QUEUE_PER_KEY,
    QPS_SHAPING_TIMEOUT, QPS_SLEEPERS_COUNT, RuntimeLimitError, RuntimeRateSnapshot,
    RuntimeRateState,
};

impl GatewayState {
    pub async fn acquire_runtime_limit(
        &self,
        gateway_key: &GatewayApiKey,
    ) -> Result<(), RuntimeLimitError> {
        let key = gateway_key.key.clone();
        let qps_limit = gateway_key.qps_limit;
        let concurrency_limit = gateway_key.concurrency_limit;

        let qps_wait = {
            let mut runtime = self.runtime_state().await;
            let qps = qps_limit.unwrap_or(0.0);
            let entry = runtime
                .entry(key.clone())
                .or_insert_with(|| RuntimeRateState {
                    last_update: Instant::now(),
                    tokens: if qps > 0.0 { (qps * 2.0).max(1.0) } else { 0.0 },
                    in_flight: 0,
                    in_queue: 0,
                    notify: std::sync::Arc::new(tokio::sync::Notify::new()),
                });

            let mut wait = Duration::ZERO;
            if let Some(qps) = qps_limit
                && qps > 0.0
            {
                let now = Instant::now();
                let elapsed = now.duration_since(entry.last_update).as_secs_f64();
                let burst = (qps * 2.0).max(1.0);
                entry.tokens = (entry.tokens + elapsed * qps).min(burst);
                entry.last_update = now;

                if entry.tokens >= 1.0 {
                    entry.tokens -= 1.0;
                } else {
                    let needed = 1.0 - entry.tokens;
                    let wait_secs = needed / qps;
                    wait = Duration::from_secs_f64(wait_secs);
                    if wait <= QPS_SHAPING_TIMEOUT {
                        entry.tokens -= 1.0;
                    } else {
                        return Err(RuntimeLimitError::QpsWaitTooLong);
                    }
                }
            }
            wait
        };

        if qps_wait > Duration::ZERO {
            let sleepers = QPS_SLEEPERS_COUNT.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            if sleepers > MAX_QPS_SLEEPERS {
                QPS_SLEEPERS_COUNT.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
                return Err(RuntimeLimitError::TooManyQpsSleepers);
            }
            tokio::time::sleep(qps_wait).await;
            QPS_SLEEPERS_COUNT.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
        }

        let notify = {
            let mut runtime = self.runtime_state().await;
            let Some(entry) = runtime.get_mut(&key) else {
                return Err(RuntimeLimitError::StateLost);
            };

            if let Some(limit) = concurrency_limit {
                if limit > 0 && (entry.in_flight as i64) >= limit {
                    if entry.in_queue >= MAX_QUEUE_PER_KEY {
                        return Err(RuntimeLimitError::QueueDepthExceeded);
                    }
                    entry.in_queue += 1;
                    entry.notify.clone()
                } else {
                    entry.in_flight += 1;
                    return Ok(());
                }
            } else {
                entry.in_flight += 1;
                return Ok(());
            }
        };

        let start = Instant::now();

        loop {
            let elapsed = start.elapsed();
            if elapsed >= CONCURRENCY_QUEUE_TIMEOUT {
                self.remove_queued_request(&key).await;
                return Err(RuntimeLimitError::QueueTimeout);
            }

            let wait_fut =
                tokio::time::timeout(CONCURRENCY_QUEUE_TIMEOUT - elapsed, notify.notified());
            if wait_fut.await.is_err() {
                self.remove_queued_request(&key).await;
                return Err(RuntimeLimitError::QueueTimeout);
            }

            let mut runtime = self.runtime_state().await;
            let Some(entry) = runtime.get_mut(&key) else {
                return Err(RuntimeLimitError::StateLost);
            };

            if let Some(limit) = concurrency_limit {
                if (entry.in_flight as i64) < limit {
                    if entry.in_queue > 0 {
                        entry.in_queue -= 1;
                    }
                    entry.in_flight += 1;
                    return Ok(());
                }
            } else {
                if entry.in_queue > 0 {
                    entry.in_queue -= 1;
                }
                entry.in_flight += 1;
                return Ok(());
            }
        }
    }

    pub async fn release_api_key_inflight(&self, key: &str) {
        let mut runtime = self.runtime_state().await;
        if let Some(entry) = runtime.get_mut(key) {
            if entry.in_flight > 0 {
                entry.in_flight -= 1;
            }
            if entry.in_queue > 0 {
                entry.notify.notify_one();
            }
        }
    }

    pub async fn queue_metrics_snapshot(&self) -> Vec<(String, RuntimeRateSnapshot)> {
        let runtime = self.runtime_state().await;
        runtime
            .iter()
            .map(|(key, entry)| {
                (
                    key.clone(),
                    RuntimeRateSnapshot {
                        tokens: entry.tokens,
                        in_flight: entry.in_flight,
                        in_queue: entry.in_queue,
                    },
                )
            })
            .collect()
    }

    async fn remove_queued_request(&self, key: &str) {
        let mut runtime = self.runtime_state().await;
        if let Some(entry) = runtime.get_mut(key)
            && entry.in_queue > 0
        {
            entry.in_queue -= 1;
        }
    }
}
