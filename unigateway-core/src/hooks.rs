use std::collections::HashMap;

use futures_util::future::BoxFuture;

use crate::pool::{EndpointId, PoolId, RequestId};
use crate::response::RequestReport;

pub trait GatewayHooks: Send + Sync + 'static {
    fn on_attempt_started(&self, event: AttemptStartedEvent) -> BoxFuture<'static, ()>;

    fn on_attempt_finished(&self, event: AttemptFinishedEvent) -> BoxFuture<'static, ()>;

    fn on_request_finished(&self, report: RequestReport) -> BoxFuture<'static, ()>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttemptStartedEvent {
    pub request_id: RequestId,
    pub pool_id: Option<PoolId>,
    pub endpoint_id: EndpointId,
    pub attempt_index: usize,
    pub metadata: HashMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttemptFinishedEvent {
    pub request_id: RequestId,
    pub endpoint_id: EndpointId,
    pub success: bool,
    pub status_code: Option<u16>,
    pub latency_ms: u64,
    pub error: Option<String>,
}
