#![allow(missing_docs)]
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;

#[derive(Debug, Clone)]
pub struct AdaptiveConcurrencyConfig {
    pub initial_concurrency: usize,
    pub max_concurrency: usize,
    pub min_concurrency: usize,
    pub cooldown_ms: u64,
}

impl Default for AdaptiveConcurrencyConfig {
    fn default() -> Self {
        Self {
            initial_concurrency: 10,
            max_concurrency: 100,
            min_concurrency: 2,
            cooldown_ms: 1000,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct AimdSnapshot {
    pub current_limit: usize,
    pub ssthresh: usize,
    pub active_connections: usize,
    pub in_cooldown_until: u64,
}

#[derive(Debug)]
pub struct AdaptiveConcurrency {
    pub config: std::sync::Arc<AdaptiveConcurrencyConfig>,
    pub current_limit: AtomicUsize,
    pub ssthresh: AtomicUsize,
    pub active_connections: AtomicUsize,
    cooldown_until: AtomicU64,
}

impl AdaptiveConcurrency {
    pub fn new(config: std::sync::Arc<AdaptiveConcurrencyConfig>) -> Self {
        Self {
            current_limit: AtomicUsize::new(config.initial_concurrency),
            ssthresh: AtomicUsize::new(config.max_concurrency),
            active_connections: AtomicUsize::new(0),
            cooldown_until: AtomicU64::new(0),
            config,
        }
    }

    pub fn snapshot(&self) -> AimdSnapshot {
        AimdSnapshot {
            current_limit: self.current_limit.load(Ordering::Relaxed),
            ssthresh: self.ssthresh.load(Ordering::Relaxed),
            active_connections: self.active_connections.load(Ordering::Relaxed),
            in_cooldown_until: self.cooldown_until.load(Ordering::Relaxed),
        }
    }

    fn now_ms() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64
    }

    pub fn acquire(self: &Arc<Self>) -> Option<AimdGuard> {
        let mut current_active = self.active_connections.load(Ordering::Relaxed);
        let limit = self.current_limit.load(Ordering::Relaxed);
        loop {
            if current_active >= limit {
                return None;
            }
            match self.active_connections.compare_exchange_weak(
                current_active,
                current_active + 1,
                Ordering::Acquire,
                Ordering::Relaxed,
            ) {
                Ok(_) => return Some(AimdGuard(self.clone())),
                Err(new_active) => current_active = new_active,
            }
        }
    }

    pub fn release(&self) {
        let _ = self.active_connections.fetch_sub(1, Ordering::Release);
    }

    pub fn on_success(&self) {
        let now = Self::now_ms();
        if now < self.cooldown_until.load(Ordering::Relaxed) {
            return;
        }

        let mut limit = self.current_limit.load(Ordering::Relaxed);
        let ssthresh = self.ssthresh.load(Ordering::Relaxed);

        loop {
            let next_limit = if limit < ssthresh {
                // Slow start: limit += 1
                limit.saturating_add(1).min(ssthresh)
            } else {
                // Congestion avoidance (simplified): AI
                limit.saturating_add(1).min(self.config.max_concurrency)
            };

            if next_limit == limit {
                break;
            }

            match self.current_limit.compare_exchange_weak(
                limit,
                next_limit,
                Ordering::SeqCst,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(new_limit) => limit = new_limit,
            }
        }
    }

    pub fn on_saturation(&self) {
        let now = Self::now_ms();
        if now < self.cooldown_until.load(Ordering::Relaxed) {
            // Already in cooldown, do nothing
            return;
        }

        let limit = self.current_limit.load(Ordering::Relaxed);
        let new_ssthresh = (limit / 2).max(self.config.min_concurrency);
        let new_limit = (limit / 2).max(self.config.min_concurrency);

        // Update ssthresh
        self.ssthresh.store(new_ssthresh, Ordering::SeqCst);

        // Multiplicative decrease
        let _ = self.current_limit.compare_exchange(
            limit,
            new_limit,
            Ordering::SeqCst,
            Ordering::Relaxed,
        );

        // Set cooldown
        self.cooldown_until.store(now + self.config.cooldown_ms, Ordering::SeqCst);
    }
}

pub struct AimdGuard(std::sync::Arc<AdaptiveConcurrency>);

impl Drop for AimdGuard {
    fn drop(&mut self) {
        self.0.release();
    }
}
