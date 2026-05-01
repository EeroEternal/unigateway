use std::collections::HashMap;
use std::time::SystemTime;

use serde::{Deserialize, Serialize};

use crate::pool::EndpointId;

/// Supplies runtime routing signals for a specific pool.
pub trait RoutingFeedbackProvider: Send + Sync + 'static {
    /// Returns the latest feedback snapshot for the requested pool.
    fn feedback(&self, pool_id: &str) -> RoutingFeedback;
}

/// Runtime signals that can influence endpoint ordering without embedding policy logic.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RoutingFeedback {
    /// Per-endpoint signals keyed by UniGateway endpoint ID.
    pub endpoint_signals: HashMap<EndpointId, EndpointSignal>,
}

/// Neutral endpoint-level routing hints that hosts can derive from external systems such as Latch.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct EndpointSignal {
    /// Higher scores rank earlier in the pre-strategy candidate order.
    ///
    /// Fallback uses this ordering directly. Random and RoundRobin may still reshuffle or rotate
    /// the resulting candidate list after suppression is applied.
    pub score: Option<f64>,
    /// Excluded endpoints are skipped when at least one non-excluded candidate remains.
    pub excluded: bool,
    /// Endpoints remain suppressed until this deadline, when present and in the future.
    pub cooldown_until: Option<SystemTime>,
    /// Optional recent error rate for host-side introspection.
    pub recent_error_rate: Option<f64>,
}
