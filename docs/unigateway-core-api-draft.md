# RFC: UniGateway Core Crate

Status: Draft

Date: 2026-04-06

Authors: UniGateway maintainers

## 1. Summary

This RFC defines the target architecture and public API direction for a future `unigateway-core` crate.

The crate is intended to serve as a reusable, database-agnostic, in-memory LLM proxy and scheduling engine. It is designed to be embedded by upper-layer systems such as OpenHub, which remain responsible for authentication, billing, control-plane storage, and routing-state production.

The core crate must remain focused on four concerns:

- accepting standardized requests and preconfigured pool targets
- selecting endpoints in memory
- executing retry and failover behavior
- producing transparent streaming and execution results

## 2. Motivation

The current UniGateway project is optimized as an end-user binary and local gateway product. That model mixes several responsibilities which are useful for the product but undesirable for a reusable crate:

- local configuration management
- admin APIs
- process operations
- product-facing CLI flows
- gateway API key lifecycle logic

For upper-layer systems such as OpenHub, the desired integration model is different.

OpenHub should own:

- authentication
- quota and billing checks
- database queries and watches
- route calculation and policy decisions
- audit and business logging

`unigateway-core` should own:

- execution against a supplied pool or candidate set
- in-memory scheduling
- retry and fallback
- stream handling
- provider protocol adaptation
- execution reporting

This split improves reuse, reduces coupling, and keeps the core crate transport- and policy-focused rather than product-focused.

## 3. Goals

This RFC proposes a core crate that:

- has zero dependency on any specific database stack
- stores runtime state purely in memory
- supports hot pool updates without process restart
- supports both chat and response-style streaming workflows
- supports embeddings in the same runtime model
- exposes its own stable public request, response, report, and plugin types
- provides built-in support for OpenAI-compatible and Anthropic-style upstreams
- allows custom drivers for non-standard upstream providers
- preserves clean execution reporting for billing and tracing use cases

## 4. Non-Goals

The initial version of `unigateway-core` does not attempt to provide:

- database clients or query code
- config-file persistence
- admin dashboards or admin APIs
- end-user authentication or API key management
- billing logic or quota enforcement
- image generation, audio, realtime, or rerank APIs
- background provider health probes
- built-in circuit-breaker systems
- secret storage backends
- `llm-connector` as a built-in core dependency

If future compatibility with `llm-connector` is desired, it should be implemented as an external optional adapter crate instead of being embedded into the core crate.

## 5. Terminology

This RFC uses the following terms:

- `Pool`: a runtime grouping of endpoints supplied by an upper-layer system
- `Endpoint`: a concrete upstream target with one base URL, one credential, one driver selection, and model policy metadata
- `Driver`: a protocol executor responsible for talking to a provider family
- `ExecutionPlan`: a per-request explicit candidate set supplied by an upper layer
- `Snapshot`: the immutable pool view captured by a request when execution starts

## 6. Normative Language

The key words “MUST”, “MUST NOT”, “SHOULD”, “SHOULD NOT”, and “MAY” in this RFC are to be interpreted as requirement levels for the design and future implementation.

## 7. Architecture Boundary

### 7.1 Core Responsibilities

`unigateway-core` MUST be responsible for:

- maintaining pure in-memory runtime state
- managing pool snapshots and endpoint lookup
- selecting endpoints according to built-in routing strategies
- applying retry and failover behavior
- executing standardized request types against registered drivers
- supporting streaming and non-streaming execution paths
- returning execution reports and usage metadata when available
- exposing asynchronous hook points for upper-layer systems

### 7.2 Out-of-Scope Responsibilities

`unigateway-core` MUST NOT be responsible for:

- database access
- control-plane persistence
- config-file lifecycle
- admin HTTP APIs
- user authentication
- quota checking
- billing enforcement
- tenant or account management

Upper-layer systems MUST own those concerns.

## 8. Zero Database Dependency

`unigateway-core` MUST have zero dependency on any specific database technology.

That includes, but is not limited to:

- PostgreSQL
- MySQL
- SQLite
- Redis
- ORM frameworks
- migration frameworks
- query builders

The crate MUST be able to operate purely from in-memory data supplied by the embedding application.

## 9. Control Plane Inversion

The engine MUST accept configuration passively from the embedding system.

The intended control flow is:

1. the upper layer reads its control plane or database
2. the upper layer materializes runtime pool definitions
3. the upper layer pushes the updated pool into the engine
4. the engine begins serving new requests from the new snapshot

The engine MUST NOT query or watch any control-plane source by itself.

## 10. State Model

The engine state MUST remain purely in memory and thread-safe.

This RFC intentionally does not require a specific internal container implementation such as `RwLock<HashMap<...>>`.

What the implementation MUST provide is:

- low-overhead in-memory reads
- thread-safe writes
- atomic pool replacement semantics
- stable per-request snapshots

Implementations MAY evolve internal data structures as long as those runtime guarantees remain true.

## 11. Packaging Model

The project SHOULD be split into two layers:

- `unigateway-core`: reusable engine crate
- `unigateway-runtime`: reusable HTTP and protocol compatibility runtime layer
- `unigateway`: product-facing CLI, admin, config, and operational layer

The responsibility boundary described in this RFC SHOULD remain stable even if internal module names continue evolving during migration.

## 12. Built-In Protocol Support

Version 1 of `unigateway-core` MUST provide built-in support for exactly two provider categories:

- `OpenAiCompatible`
- `Anthropic`

These two built-in drivers are expected to cover the majority of upstream integrations.

## 13. Core Must Not Depend on `llm-connector`

`unigateway-core` MUST NOT depend on `llm-connector` as part of its core design.

The rationale is straightforward:

- the core crate should own its execution abstractions directly
- the public API should not be shaped by a third-party provider abstraction layer
- long-tail provider complexity should not leak into the engine core

If future `llm-connector` interoperability is needed, it SHOULD be implemented as a separate optional adapter crate.

## 14. Prefer Upper-Layer Normalization

When a provider can be normalized by an upper layer into a standard OpenAI-compatible or Anthropic-compatible request, that normalization SHOULD happen before calling `unigateway-core`.

The preferred order of integration is:

1. use the built-in OpenAI-compatible driver
2. use the built-in Anthropic driver
3. normalize provider quirks in the upper layer when possible
4. only introduce a custom driver when standardization outside the engine is not realistic

This keeps the core crate narrow, stable, and easier to reason about.

## 15. Execution Modes

The engine MUST support two execution modes:

- execution by `pool_id`
- execution by an explicit `ExecutionPlan`

Execution by `pool_id` is intended for ordinary embedded use.

Execution by `ExecutionPlan` is intended for upper layers that want to precompute a candidate queue or apply custom policy per request.

## 16. Version 1 Capability Scope

Version 1 of the core crate MUST include:

- chat
- responses
- embeddings

Other endpoint families are outside the scope of this RFC.

## 17. Routing and Retry Requirements

### 17.1 Built-In Routing Strategies

Version 1 MUST provide exactly two built-in routing strategies:

- `Random`
- `RoundRobin`

The design SHOULD preserve room for future plugin-style routing extensions.

Version 1 SHOULD NOT freeze a public routing strategy extension trait unless a concrete extension need emerges.

### 17.2 Retry Surface

Version 1 retry behavior MUST only cover:

- HTTP `429`
- HTTP `500..=599`
- timeout failures
- transport failures

Other error classes SHOULD be treated as non-retriable by default.

### 17.3 Streaming Retry Boundary

Transparent retry MUST NOT occur after the first downstream stream event has been emitted.

Retry and failover are only valid before downstream streaming output begins.

## 18. Snapshot Semantics

Pool updates MUST use snapshot semantics.

Specifically:

- new requests MUST observe the most recently committed pool snapshot
- in-flight requests MUST continue using the snapshot they started with
- later pool updates MUST NOT mutate the candidate set of an already-running request

This behavior is required for predictable concurrency and safe hot updates.

## 19. Driver Model

### 19.1 Endpoint Data Must Stay Pure

`Endpoint` MUST remain a pure data structure.

It MUST NOT directly store `Arc<dyn ProviderDriver>` or any equivalent behavior object.

Instead:

- `Endpoint` MUST store `provider_kind`
- `Endpoint` MUST store `driver_id`
- the engine MUST resolve behavior through a driver registry

This keeps pool snapshots comparable, serializable, and replaceable.

### 19.2 Public Driver Traits Must Not Expose `reqwest`

The public plugin boundary MUST NOT expose `reqwest::Request` or `reqwest::Response`.

Driver traits MUST consume and produce core-owned stable types.

This prevents transport implementation details from becoming permanent API constraints.

### 19.3 Driver Usage Intent

Custom drivers SHOULD be reserved for providers that cannot be reasonably normalized by the upper layer into OpenAI-compatible or Anthropic-compatible execution.

Examples include:

- custom signing requirements
- non-standard authentication headers
- incompatible streaming wire formats

## 20. Streaming and Result Model

Streaming execution MUST use a dual-return model.

That means a streaming request returns:

- a stream that can be forwarded immediately
- a completion handle that resolves after upstream completion

The completion handle MUST carry the final result and execution report when available.

This is required because usage data often becomes available only after the stream has fully completed.

## 21. Observability and Reporting

The engine MUST return execution results that expose:

- request identity
- selected endpoint identity
- attempt trail
- usage when available
- total latency
- caller-provided metadata

The result model MUST expose `selected_endpoint_id` and MUST NOT expose raw upstream secret values.

## 22. Hooks

The engine MUST provide asynchronous hook points.

Hooks are intended for:

- billing record creation
- trace logging
- metrics emission
- audit and risk integration

Hook failures SHOULD NOT fail the proxy request itself.

Implementations MAY surface hook failure information as diagnostics, but the main request path SHOULD remain independent from hook success.

## 23. Public API Draft

The following Rust signatures are an illustrative API draft for the design described by this RFC.

```rust
use std::collections::HashMap;
use std::time::{Duration, SystemTime};

pub type PoolId = String;
pub type EndpointId = String;
pub type DriverId = String;
pub type RequestId = String;

pub struct UniGatewayEngine {
    // internal state
}

impl UniGatewayEngine {
    pub fn builder() -> UniGatewayEngineBuilder;

    pub async fn upsert_pool(&self, pool: ProviderPool) -> Result<(), GatewayError>;

    pub async fn remove_pool(&self, pool_id: &str) -> Result<(), GatewayError>;

    pub async fn get_pool(&self, pool_id: &str) -> Option<ProviderPool>;

    pub async fn list_pools(&self) -> Vec<PoolSummary>;

    pub async fn proxy_chat(
        &self,
        request: ProxyChatRequest,
        target: ExecutionTarget,
    ) -> Result<ProxySession<ChatResponseChunk, ChatResponseFinal>, GatewayError>;

    pub async fn proxy_responses(
        &self,
        request: ProxyResponsesRequest,
        target: ExecutionTarget,
    ) -> Result<ProxySession<ResponsesEvent, ResponsesFinal>, GatewayError>;

    pub async fn proxy_embeddings(
        &self,
        request: ProxyEmbeddingsRequest,
        target: ExecutionTarget,
    ) -> Result<CompletedResponse<EmbeddingsResponse>, GatewayError>;
}

pub struct UniGatewayEngineBuilder {
    pub hooks: Option<std::sync::Arc<dyn GatewayHooks>>,
    pub driver_registry: Option<std::sync::Arc<dyn DriverRegistry>>,
    pub default_retry_policy: RetryPolicy,
    pub default_timeout: Option<Duration>,
}

#[derive(Debug, Clone)]
pub enum ExecutionTarget {
    Pool {
        pool_id: PoolId,
    },
    Plan(ExecutionPlan),
}

#[derive(Debug, Clone)]
pub struct ExecutionPlan {
    pub pool_id: Option<PoolId>,
    pub candidates: Vec<EndpointRef>,
    pub load_balancing_override: Option<LoadBalancingStrategy>,
    pub retry_policy_override: Option<RetryPolicy>,
    pub metadata: HashMap<String, String>,
}

#[derive(Debug, Clone)]
pub struct EndpointRef {
    pub endpoint_id: EndpointId,
}

#[derive(Debug, Clone)]
pub struct ProviderPool {
    pub pool_id: PoolId,
    pub endpoints: Vec<Endpoint>,
    pub load_balancing: LoadBalancingStrategy,
    pub retry_policy: RetryPolicy,
    pub metadata: HashMap<String, String>,
}

#[derive(Debug, Clone)]
pub struct Endpoint {
    pub endpoint_id: EndpointId,
    pub provider_kind: ProviderKind,
    pub driver_id: DriverId,
    pub base_url: String,
    pub api_key: SecretString,
    pub model_policy: ModelPolicy,
    pub enabled: bool,
    pub metadata: HashMap<String, String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderKind {
    OpenAiCompatible,
    Anthropic,
}

#[derive(Debug, Clone)]
pub struct ModelPolicy {
    pub default_model: Option<String>,
    pub model_mapping: HashMap<String, String>,
}

pub trait DriverRegistry: Send + Sync + 'static {
    fn get(&self, driver_id: &str) -> Option<std::sync::Arc<dyn ProviderDriver>>;
}

pub trait ProviderDriver: Send + Sync + 'static {
    fn driver_id(&self) -> &str;

    fn provider_kind(&self) -> ProviderKind;

    fn execute_chat(
        &self,
        endpoint: DriverEndpointContext,
        request: ProxyChatRequest,
    ) -> futures_util::future::BoxFuture<'static, Result<ProxySession<ChatResponseChunk, ChatResponseFinal>, GatewayError>>;

    fn execute_responses(
        &self,
        endpoint: DriverEndpointContext,
        request: ProxyResponsesRequest,
    ) -> futures_util::future::BoxFuture<'static, Result<ProxySession<ResponsesEvent, ResponsesFinal>, GatewayError>>;

    fn execute_embeddings(
        &self,
        endpoint: DriverEndpointContext,
        request: ProxyEmbeddingsRequest,
    ) -> futures_util::future::BoxFuture<'static, Result<CompletedResponse<EmbeddingsResponse>, GatewayError>>;
}

#[derive(Debug, Clone)]
pub struct DriverEndpointContext {
    pub endpoint_id: EndpointId,
    pub provider_kind: ProviderKind,
    pub base_url: String,
    pub api_key: SecretString,
    pub model_policy: ModelPolicy,
    pub metadata: HashMap<String, String>,
}

#[derive(Debug, Clone)]
pub struct ProxyChatRequest {
    pub model: String,
    pub messages: Vec<Message>,
    pub temperature: Option<f32>,
    pub top_p: Option<f32>,
    pub max_tokens: Option<u32>,
    pub stream: bool,
    pub metadata: HashMap<String, String>,
}

#[derive(Debug, Clone)]
pub struct ProxyResponsesRequest {
    pub model: String,
    pub input: serde_json::Value,
    pub stream: bool,
    pub metadata: HashMap<String, String>,
}

#[derive(Debug, Clone)]
pub struct ProxyEmbeddingsRequest {
    pub model: String,
    pub input: Vec<String>,
    pub metadata: HashMap<String, String>,
}

#[derive(Debug, Clone)]
pub struct Message {
    pub role: MessageRole,
    pub content: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageRole {
    System,
    User,
    Assistant,
    Tool,
}

pub enum ProxySession<Chunk, Final> {
    Completed(CompletedResponse<Final>),
    Streaming(StreamingResponse<Chunk, Final>),
}

pub struct CompletedResponse<T> {
    pub response: T,
    pub report: RequestReport,
}

pub struct StreamingResponse<Chunk, Final> {
    pub stream: ResponseStream<Chunk>,
    pub completion: CompletionHandle<Final>,
    pub request_id: RequestId,
}

pub type ResponseStream<T> =
    std::pin::Pin<Box<dyn futures_core::Stream<Item = Result<T, GatewayError>> + Send>>;

pub type CompletionHandle<T> =
    tokio::sync::oneshot::Receiver<Result<CompletedResponse<T>, GatewayError>>;

#[derive(Debug, Clone)]
pub struct RequestReport {
    pub request_id: RequestId,
    pub pool_id: Option<PoolId>,
    pub selected_endpoint_id: EndpointId,
    pub selected_provider: ProviderKind,
    pub attempts: Vec<AttemptReport>,
    pub usage: Option<TokenUsage>,
    pub latency_ms: u64,
    pub started_at: SystemTime,
    pub finished_at: SystemTime,
    pub metadata: HashMap<String, String>,
}

#[derive(Debug, Clone)]
pub struct AttemptReport {
    pub endpoint_id: EndpointId,
    pub status: AttemptStatus,
    pub latency_ms: u64,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttemptStatus {
    Succeeded,
    Failed,
    Retried,
}

#[derive(Debug, Clone)]
pub struct TokenUsage {
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub total_tokens: Option<u64>,
}

#[derive(Debug, Clone)]
pub enum LoadBalancingStrategy {
    Random,
    RoundRobin,
}

#[derive(Debug, Clone)]
pub struct RetryPolicy {
    pub max_attempts: usize,
    pub per_attempt_timeout: Option<Duration>,
    pub retry_on: Vec<RetryCondition>,
    pub backoff: BackoffPolicy,
    pub stop_after_stream_started: bool,
}

#[derive(Debug, Clone)]
pub enum RetryCondition {
    HttpStatus(u16),
    HttpStatusRange { start: u16, end: u16 },
    Timeout,
    TransportError,
}

#[derive(Debug, Clone)]
pub enum BackoffPolicy {
    None,
    Fixed(Duration),
    Exponential {
        base: Duration,
        max: Duration,
        jitter: bool,
    },
}

pub trait GatewayHooks: Send + Sync + 'static {
    fn on_attempt_started(
        &self,
        event: AttemptStartedEvent,
    ) -> futures_util::future::BoxFuture<'static, ()>;

    fn on_attempt_finished(
        &self,
        event: AttemptFinishedEvent,
    ) -> futures_util::future::BoxFuture<'static, ()>;

    fn on_request_finished(
        &self,
        report: RequestReport,
    ) -> futures_util::future::BoxFuture<'static, ()>;
}

pub struct AttemptStartedEvent {
    pub request_id: RequestId,
    pub pool_id: Option<PoolId>,
    pub endpoint_id: EndpointId,
    pub attempt_index: usize,
    pub metadata: HashMap<String, String>,
}

pub struct AttemptFinishedEvent {
    pub request_id: RequestId,
    pub endpoint_id: EndpointId,
    pub success: bool,
    pub status_code: Option<u16>,
    pub latency_ms: u64,
    pub error: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum GatewayError {
    #[error("pool not found: {0}")]
    PoolNotFound(String),

    #[error("endpoint not found: {0}")]
    EndpointNotFound(String),

    #[error("invalid request: {0}")]
    InvalidRequest(String),

    #[error("no available endpoint")]
    NoAvailableEndpoint,

    #[error("all attempts failed")]
    AllAttemptsFailed {
        attempts: Vec<AttemptReport>,
    },

    #[error("upstream http error: {status}")]
    UpstreamHttp {
        status: u16,
        body: Option<String>,
        endpoint_id: EndpointId,
    },

    #[error("transport error: {message}")]
    Transport {
        message: String,
        endpoint_id: Option<EndpointId>,
    },

    #[error("stream aborted: {message}")]
    StreamAborted {
        message: String,
        endpoint_id: EndpointId,
    },
}
```

## 24. Suggested Internal Module Layout

The internal implementation MAY evolve, but the design described by this RFC suggests a module structure similar to:

```text
unigateway-core/
  src/
    lib.rs
    engine.rs
    pool.rs
    routing.rs
    retry.rs
    request.rs
    response.rs
    hooks.rs
    error.rs
    drivers.rs
    registry.rs
    transport.rs
    protocol/
      mod.rs
      openai.rs
      anthropic.rs
```

## 25. Open Questions

The core direction is now mostly settled, but the following questions remain open for final design lock.

### 25.1 Pool Mutation API Shape

Should `upsert_pool` remain the only mutation primitive, or should the crate also provide convenience APIs such as:

- `upsert_endpoint`
- `remove_endpoint`
- `set_endpoint_enabled`

Current recommendation: keep `upsert_pool` as the primary atomic mutation API.

### 25.2 Secret Read Semantics

Should read-path APIs return redacted endpoint data, or should secret-bearing fields be omitted from read responses entirely?

Current recommendation: read APIs should redact or omit raw secret values.

### 25.3 Embeddings Failover Semantics

Should embeddings follow the same retry and failover model as chat and responses, or should embeddings be restricted to a simpler execution path?

Current recommendation: use the same retry model unless a concrete incompatibility is discovered.

### 25.4 HTTP Layer Packaging Name

Should the HTTP-facing layer become a distinct `unigateway-runtime` crate, or should the current `unigateway` crate continue to own protocol compatibility directly?

Current recommendation: introduce `unigateway-runtime` as the reusable gateway layer above `unigateway-core`, and keep `unigateway` as the end-user product package built on top of both.
