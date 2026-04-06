pub mod drivers;
pub mod engine;
pub mod error;
pub mod hooks;
pub mod pool;
pub mod protocol;
pub mod registry;
pub mod request;
pub mod response;
pub mod retry;
pub mod routing;
pub mod transport;

pub use drivers::{DriverEndpointContext, DriverRegistry, ProviderDriver};
pub use engine::{UniGatewayEngine, UniGatewayEngineBuilder};
pub use error::GatewayError;
pub use hooks::{AttemptFinishedEvent, AttemptStartedEvent, GatewayHooks};
pub use pool::{
    DriverId, Endpoint, EndpointId, EndpointRef, ExecutionPlan, ExecutionTarget, ModelPolicy,
    PoolId, PoolSummary, ProviderKind, ProviderPool, RequestId, SecretString,
};
pub use registry::InMemoryDriverRegistry;
pub use request::{
    Message, MessageRole, ProxyChatRequest, ProxyEmbeddingsRequest, ProxyResponsesRequest,
};
pub use response::{
    AttemptReport, AttemptStatus, ChatResponseChunk, ChatResponseFinal, CompletedResponse,
    CompletionHandle, EmbeddingsResponse, ProxySession, RequestReport, ResponseStream,
    ResponsesEvent, ResponsesFinal, StreamingResponse, TokenUsage,
};
pub use retry::{BackoffPolicy, LoadBalancingStrategy, RetryCondition, RetryPolicy};
