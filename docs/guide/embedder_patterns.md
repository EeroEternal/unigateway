# Embedder Patterns

本文件描述将 UniGateway 嵌入高性能生产环境时的常见集成模式，适用于 Nebula 等需要感知集群状态、外置调度决策的宿主应用。

> **前置阅读**：[`embed.md`](embed.md)（基础嵌入流程）、[`arch.md`](../design/arch.md)（架构总览）。

---

## 模式一：响应式 `PoolHost`（动态状态感知）

默认情况下，`PoolHost` 从引擎内存中的 pool 映射读取数据，数据源是 TOML 配置文件。生产环境中的端点状态（权重、熔断、负载）变化频繁，需要通过 `PoolHost` 的实现将外部状态引入。

### 核心思路

```
外部状态存储（etcd / 控制面 API）
        │
        ▼  push 或周期拉取
   本地缓存（Arc<DashMap> / Arc<RwLock<HashMap>>）
        │
        ▼  PoolHost::pool_for_service()
   UniGatewayEngine（内存 pool 只读）
```

### 实现示例

```rust
use std::collections::HashMap;
use std::sync::Arc;
use unigateway_core::{ProviderPool, UniGatewayEngine};
use unigateway_host::{
    EnvPoolHost, HostContext, HostFuture, PoolHost, PoolLookupError, PoolLookupOutcome,
    PoolLookupResult,
};

/// 本地缓存：由控制面异步刷新。
struct PoolCache {
    inner: Arc<dashmap::DashMap<String, ProviderPool>>,
}

impl PoolHost for PoolCache {
    fn pool_for_service<'a>(
        &'a self,
        service_id: &'a str,
    ) -> HostFuture<'a, PoolLookupResult<PoolLookupOutcome>> {
        Box::pin(async move {
            let pool = self
                .inner
                .get(service_id)
                .map(|r| r.clone())
                .ok_or_else(|| PoolLookupError::other("pool not found in cache"))?;
            Ok(PoolLookupOutcome::found(pool))
        })
    }
}

// EnvPoolHost 可选实现（env fallback）：
impl EnvPoolHost for PoolCache {
    fn env_pool<'a>(
        &'a self,
        _provider: EnvProvider,
        _api_key_override: Option<&'a str>,
    ) -> HostFuture<'a, PoolLookupResult<PoolLookupOutcome>> {
        Box::pin(async { Ok(PoolLookupOutcome::not_found()) })
    }
}
```

### 缓存更新策略

| 策略 | 适用场景 | 注意事项 |
|------|---------|---------|
| 控制面 push（推荐） | 状态变化低频、需要近实时 | 在控制面变更时调用 `engine.upsert_pool(pool)` 刷新引擎内存 |
| 周期拉取 | 控制面无 push 能力 | 注意拉取间隔内的状态lag；拉取时不阻塞请求线程 |
| 请求时 pull | 状态强一致要求极高 | **不推荐**——每次请求访问 etcd 会引入显著延迟 |

### 端点级动态字段更新

当只需要更新某个端点的元数据（如权重、熔断状态），不需要重建整个 pool：

```rust
/// 由控制面在状态变更时调用
async fn refresh_endpoint_metadata(
    engine: &UniGatewayEngine,
    pool_id: &str,
    endpoint_id: &str,
    metadata: HashMap<String, String>,
) -> anyhow::Result<()> {
    // 读取 → 修改 → 写回
    let mut pool = engine
        .get_pool(pool_id)
        .await
        .ok_or_else(|| anyhow::anyhow!("pool not found: {}", pool_id))?;

    for ep in &mut pool.endpoints {
        if ep.endpoint_id == endpoint_id {
            ep.metadata.extend(metadata);
            break;
        }
    }

    engine.upsert_pool(pool).await?;
    Ok(())
}
```

> **注意**：当前 `UniGatewayEngine` 尚未提供单端点元数据更新的专用 API。上述 read-modify-write 模式是目前的推荐做法。未来版本可考虑增加一个 `update_endpoint_metadata` 方法以降低开销。

---

## 模式二：外置路由决策（Explicit Routing）

UniGateway 的内置路由策略（round_robin / random / fallback）在简单场景下足够。当调度决策由外部系统（如 Nebula 调度器）做出时，应跳过内置策略，直接构造目标 pool 传给 dispatch。

### `HostDispatchTarget` 的三个变体

```rust
pub enum HostDispatchTarget<'a> {
    Service(&'a str),        // ← 内置路由：由 PoolHost 解析
    Pool(ProviderPool),       // ← 外置路由：调用者构造完整 pool
    PoolRef(&'a ProviderPool), // ← 外置路由：引用已有 pool（零拷贝）
}
```

### 外置路由示例

```rust
use unigateway_core::{Endpoint, ProviderPool, LoadBalancingStrategy, RetryPolicy};
use unigateway_host::core::dispatch::{HostDispatchTarget, dispatch_request, HostProtocol, HostRequest};

/// Nebula 调度器：根据集群实时状态选择端点
fn nebula_schedule(service_id: &str) -> anyhow::Result<HostDispatchTarget<'static>> {
    // 1. 调用 Nebula 调度 API 获取选中的端点
    let selected_endpoint = nebula_client::select_endpoint(service_id)?;

    // 2. 构造只含一个端点的 Pool（相当于显式路由）
    let pool = ProviderPool {
        pool_id:        format!("nebula:{service_id}"),
        load_balancing: LoadBalancingStrategy::RoundRobin,
        retry_policy:   RetryPolicy::default(),
        metadata:        HashMap::new(),
        endpoints:       vec![selected_endpoint],
    };

    // 3. 以 Pool 变体传入，跳过内置路由
    Ok(HostDispatchTarget::Pool(pool))
}

// 使用：
let target = nebula_schedule("chat-fast")?;
let outcome = dispatch_request(
    &host_context,
    target,
    HostProtocol::OpenAiChat,
    None,
    HostRequest::Chat(request),
).await?;
```

### 决策点

| 场景 | 推荐做法 |
|------|---------|
| 调度器返回完整端点列表（已排序） | 构造 `Pool`，endpoints 按调度器顺序填充，`load_balancing` 设为 `RoundRobin`（尊重调度器顺序） |
| 调度器只返回一个端点 | 构造只含一个端点的 `Pool`，失败时由 UniGateway 的 `RetryPolicy` 控制是否重试其他端点 |
| 调度器返回 pool_id，由 UniGateway 内置策略选端点 | 使用 `HostDispatchTarget::Service(pool_id)`，让 `PoolHost` 解析 |

---

## 模式三：使用 `GatewayHooks`（请求修改、流式观测与审计）

`GatewayHooks` 现在已经同时覆盖请求修改、生命周期观测和流式 chunk 观测。它适合做
消费者应用自有的请求富化、trace 注入、审计和 metrics 采集，但不适合承载大段 provider
解析逻辑。

### 现有 `GatewayHooks`

```rust
pub trait GatewayHooks: Send + Sync + 'static {
    fn on_request_started(&self, event: RequestStartedEvent) -> BoxFuture<'static, ()>;
    fn on_attempt_started(&self, event: AttemptStartedEvent) -> BoxFuture<'static, ()>;
    fn on_attempt_finished(&self, event: AttemptFinishedEvent) -> BoxFuture<'static, ()>;
    fn on_request_finished(&self, report: RequestReport) -> BoxFuture<'static, ()>;
    fn on_stream_started(&self, event: StreamStartedEvent) -> BoxFuture<'static, ()>;
    fn on_stream_chunk_event(&self, event: StreamChunkEvent) -> BoxFuture<'static, ()>;
    fn on_stream_completed(&self, report: StreamReport) -> BoxFuture<'static, ()>;
    fn on_stream_aborted(&self, report: StreamReport) -> BoxFuture<'static, ()>;
    fn on_request(&self, req: &mut ProxyChatRequest) -> BoxFuture<'static, ()>;
    fn on_stream_chunk(&self, chunk: &ChatResponseChunk) -> BoxFuture<'static, ()>;
}
```

### 使用场景示例

**注入自定义 Header（通过 metadata 透传）**：

```rust
struct HeaderInjectionHooks {
    inner: Arc<dyn GatewayHooks>,
}

impl GatewayHooks for HeaderInjectionHooks {
    // 转发现有 hook 调用
    fn on_attempt_started(&self, event: AttemptStartedEvent) -> BoxFuture<'static, ()> {
        self.inner.on_attempt_started(event)
    }
    // ... 其他方法类似转发 ...

    fn on_request(&self, req: &mut ProxyChatRequest) -> BoxFuture<'static, ()> {
        Box::pin(async move {
            // 从 metadata 中提取需要透传到 upstream 的字段
            if let Some(trace_id) = req.metadata.remove("trace_id") {
                // 实际实现中可通过 DriverEndpointContext 或自定义 transport 透传
                println!("trace_id: {}", trace_id);
            }
        })
    }
}
```

### 什么时候不要用 `GatewayHooks`

如果你需要做的是 provider-specific 上游协议解析，例如把某家 provider 的私有 chunk 先转成
标准的 OpenAI / Anthropic 形状，再交给 UniGateway 渲染，那么 `GatewayHooks` 不是合适层。
这类逻辑更适合：

- 在消费者应用里维护 provider profile，并投影为 endpoint/request metadata；
- 或者直接包一层 custom driver / adapter。

`GatewayHooks` 更适合“修改请求”和“观测执行”，不适合承担完整的 provider normalizer。

**审计日志**：

```rust
impl GatewayHooks for AuditHooks {
    fn on_request_finished(&self, report: RequestReport) -> BoxFuture<'static, ()> {
        Box::pin(async move {
            audit_log::record(
                report.request_id,
                report.usage,
                report.latency_ms,
                report.metadata,
            );
        })
    }

    fn on_stream_chunk(&self, chunk: &ChatResponseChunk) -> BoxFuture<'static, ()> {
        Box::pin(async move {
            if let Some(ref text) = chunk.delta {
                audit_log::record_token(chunk.endpoint_id.clone(), text.len());
            }
        })
    }
}
```

---

## 模式四：运行时 Pool/Endpoint 更新（无重启刷新）

生产环境中，端点的启用/禁用、权重调整、模型策略变更不应要求重启进程。

### 推荐流程

```
控制面发出变更事件
        │
        ▼
   嵌入者收到事件（mpsc channel / etcd watch / webhook）
        │
        ▼
   构造或更新 ProviderPool / Endpoint
        │
        ▼
   调用 engine.upsert_pool(updated_pool)
```

### 部分更新示例

```rust
/// 禁用某个端点（如熔断）
async fn disable_endpoint(
    engine: &UniGatewayEngine,
    pool_id: &str,
    endpoint_id: &str,
) -> anyhow::Result<()> {
    let mut pool = engine
        .get_pool(pool_id)
        .await
        .ok_or_else(|| anyhow::anyhow!("pool not found"))?;

    let mut found = false;
    for ep in &mut pool.endpoints {
        if ep.endpoint_id == endpoint_id {
            ep.enabled = false;
            found = true;
            break;
        }
    }

    if !found {
        anyhow::bail!("endpoint {} not found in pool {}", endpoint_id, pool_id);
    }

    engine.upsert_pool(pool).await?;
    Ok(())
}

/// 更新端点权重（用于负载均衡）
async fn update_endpoint_weight(
    engine: &UniGatewayEngine,
    pool_id: &str,
    endpoint_id: &str,
    weight: u32,
) -> anyhow::Result<()> {
    let mut pool = engine
        .get_pool(pool_id)
        .await
        .ok_or_else(|| anyhow::anyhow!("pool not found"))?;

    for ep in &mut pool.endpoints {
        if ep.endpoint_id == endpoint_id {
            ep.metadata.insert("weight".to_string(), weight.to_string());
            break;
        }
    }

    engine.upsert_pool(pool).await?;
    Ok(())
}
```

> **未来增强**：`UniGatewayEngine` 可以增加细粒度 API（`update_endpoint_metadata`、`enable_endpoint` 等），避免 read-modify-write 全量替换的开销。

---

## 集成检查清单

嵌入 UniGateway 到生产环境时，按以下清单确认：

- [ ] `UniGatewayEngine::with_builtin_http_drivers()` 或自定义 `DriverRegistry` 已配置
- [ ] `GatewayHooks` 已实现并挂载（观测、审计、请求/响应修改）
- [ ] `PoolHost` 已实现，数据源与刷新策略已明确（push / 周期拉取）
- [ ] 路由决策外置时，使用 `HostDispatchTarget::Pool(...)` 构造目标
- [ ] 运行时更新走 `engine.upsert_pool()`，不走 TOML 重刷
- [ ] `cargo test --workspace` 通过
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` 无告警

---

## 参考：Nebula 集成案例

Nebula 作为推理编排平台，对 UniGateway 的集成需求如下：

| 需求 | 使用模式 | 说明 |
|------|---------|------|
| 感知集群实时状态 | 模式一 | 实现 `PoolHost`，内部持有 etcd watch 产生的本地缓存 |
| 外置调度决策 | 模式二 | 调度器构造单端点 `Pool`，通过 `HostDispatchTarget::Pool(...)` 传入 |
| 请求审计 | 模式三 | 实现 `GatewayHooks::on_request_finished` 记录审计日志 |
| 无重启更新端点状态 | 模式四 | etcd watch 触发 `engine.upsert_pool()` 或细粒度端点更新 |

Nebula 不应修改 UniGateway 核心代码，所有集成均通过实现 trait 和调用公开 API 完成。

---

## 模式五：Host middleware（请求变更 / gateway 字段）

`GatewayHooks` 用于观测 request/attempt 生命周期；若需要在 `proxy_chat` 之前**修改** `ProxyChatRequest`（例如 delta 组装、审计注入），使用 opt-in 的 host middleware：

```rust
use std::sync::Arc;
use unigateway_host::{
    ChatRequestMiddleware, HostContext, HostMiddleware, HostFuture, HostResult,
    dispatch_request_with_middleware, HostDispatchTarget, HostProtocol, HostRequest,
};

struct DeltaMiddleware;

impl ChatRequestMiddleware for DeltaMiddleware {
    fn on_chat_request<'a>(
        &'a self,
        _ctx: &'a HostContext<'_>,
        request: &'a mut ProxyChatRequest,
        gateway_fields: &'a HashMap<String, Value>,
    ) -> HostFuture<'a, HostResult<()>> {
        Box::pin(async move {
            if let Some(ctx) = gateway_fields.get("_session_context") {
                // 读取 gateway_fields，组装 messages …
                let _ = ctx;
            }
            Ok(())
        })
    }
}

let middleware = HostMiddleware::new().with_request(Arc::new(DeltaMiddleware));
dispatch_request_with_middleware(
    &context,
    target,
    HostProtocol::OpenAiChat,
    None,
    HostRequest::Chat(request),
    Some(&middleware),
)
.await?;
```

推荐顺序：protocol parse →（可选 session 组装）→ middleware → `proxy_chat` → render。详见 [`embedder-neutral-extensions.md`](../design/embedder-neutral-extensions.md)。

### 模式六：Metadata → HTTP header 转发（R3，opt-in）

静态 outbound 头用 endpoint `metadata` 的 `http_header.*` 前缀（见 core OpenAI driver）。**Per-request** 头转发需配置 allowlist：

```rust
Endpoint {
    // ...
    forward_metadata_as_headers: Some(vec![
        "X-Tenant-Id".to_string(),
        "X-Custom-*".to_string(),
    ]),
    ..endpoint
}
```

仅 `ProxyChatRequest.metadata` 中匹配 allowlist 的键会变为 outbound header；`unigateway.*` 默认不转发。Pool 级 allowlist 与 endpoint 合并。

详见 [`protocol-conversion.md`](../design/protocol-conversion.md#outbound-http-headers)。

### 模式七：Session prefix 参考实现（R5，opt-in）

若 embedder 需要 namespace 隔离、epoch 单调 publish、raw JSON delta 组装、fingerprint/TTL 等能力，可使用可选 crate `unigateway-session`（非默认依赖）：

```toml
unigateway-sdk = { version = "2.12", features = ["host", "session"] }
unigateway-session = { version = "2.12", features = ["http"] }
```

```rust
use std::sync::Arc;
use unigateway_session::{
    DeltaAssemblyMiddleware, MemorySessionStore, SessionKey, SessionMiddlewareConfig,
    SessionStoreConfig, SessionLifetime, FingerprintPolicy,
};
use unigateway_host::HostMiddleware;

let store = Arc::new(MemorySessionStore::with_config(SessionStoreConfig {
    lifetime: SessionLifetime {
        touch_on_read: true,
        ..Default::default()
    },
    ..Default::default()
}));
let middleware = HostMiddleware::new().with_request(Arc::new(
    DeltaAssemblyMiddleware::with_store(
        store.clone(),
        SessionMiddlewareConfig {
            fingerprint_policy: FingerprintPolicy::Optional,
            ..Default::default()
        },
    ),
));
// 客户端 body 需保留 raw messages；gateway_fields 示例:
// "_session_context": {"session_id":"s1","epoch":1,"delivery":"delta","tail_start":1}
// namespace 由宿主 SessionKey resolver 注入，不可由客户端 body 信任
```

`http` feature 提供 `POST/DELETE /v1/gateway/sessions/{id}/publish|delete`（`SessionHttpConfig.namespace` 可配置租户隔离）。自有 session 存储的产品可只用 R1+R2+R4，不必启用本 crate。

详见 [`unigateway-session/README.md`](../../unigateway-session/README.md) 与 [`docs/dev/issues/04-session-generalization-p0.md`](../dev/issues/04-session-generalization-p0.md)。
