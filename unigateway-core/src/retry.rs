use std::time::Duration;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum LoadBalancingStrategy {
    Fallback,
    Random,
    RoundRobin,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RetryCondition {
    HttpStatus(u16),
    HttpStatusRange { start: u16, end: u16 },
    Timeout,
    TransportError,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum BackoffPolicy {
    None,
    Fixed(Duration),
    Exponential {
        base: Duration,
        max: Duration,
        jitter: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RetryPolicy {
    pub max_attempts: usize,
    pub per_attempt_timeout: Option<Duration>,
    pub retry_on: Vec<RetryCondition>,
    pub backoff: BackoffPolicy,
    pub stop_after_stream_started: bool,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_attempts: 1,
            per_attempt_timeout: None,
            retry_on: Vec::new(),
            backoff: BackoffPolicy::None,
            stop_after_stream_started: true,
        }
    }
}
