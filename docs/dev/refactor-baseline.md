# 重构基线：分层定位与抽象余地

> **仓库形态注记（2026-04）**：独立 `ug` binary、根目录 `src/` 与 `unigateway-cli` 已移除；本文中关于根 crate、`src/*`、CLI 的段落均为**历史拆分过程记录**。当前事实来源以 [`../design/arch.md`](../design/arch.md)、[`memory.md`](./memory.md) 顶部说明与根 `Cargo.toml` workspace 成员为准。

本文件是对某一阶段 UniGateway workspace 分层（`unigateway-core` / `unigateway-host` / `unigateway-protocol` / `unigateway-config`、以及当时尚存在的根产品 crate）的一次整体体检，作为后续重构工作的基础参考。

- 配合阅读：[`memory.md`](./memory.md)（快速心智模型）、[`../design/arch.md`](../design/arch.md)（当前架构描述）。
- 本文不描述"应然"的终态架构，而是盘点**当前实现**中的耦合点、裂缝与抽象机会，按 ROI 给出建议顺序。

> **进度注记（2026-04-19）**：`unigateway-runtime` → `unigateway-host` 的物理重命名已经完成，host contract 的公开符号也已同步收敛到 `Host*` 前缀。本文现在直接描述当前代码，不再使用“目标命名”的占位写法。

> **进度注记（2026-04-19）**：`unigateway-config` 的第一刀物理抽取已经落地：原 `src/config.rs` + `src/config/*` 已迁入独立 workspace crate，根 crate 当前只保留一个薄 `src/config.rs` re-export 兼容层；`resolve_upstream` / `normalize_base_url` 也已迁入 `unigateway-config::routing`，根 crate 的 `routing.rs` 收缩为请求 hint 抽取与兼容 re-export。

> **进度注记（2026-04-19）**：host contract 的第二轮收口已经完成：`EngineHost` 已删除，env fallback 已从主 `PoolHost` contract 中拆到独立的 `EnvPoolHost` / `EnvProvider` 子模块，host 公开执行入口也已收束成更中立的 dispatch API。protocol crate 的中立响应类型已从 `RuntimeHttpResponse` 更名为 `ProtocolHttpResponse`。

## 0. 当前阶段总结

下面这部分用于替代原先独立的 `refactor-summary.md`，作为本轮已完成项、当前边界和剩余结构债的单一事实来源。

### 0.1 已完成的结构调整

**workspace crate 拆分**

已经完成的物理拆分包括：

- `unigateway-config`
   - 承接配置持久化、admin mutation、路由辅助、config -> core pool 投影。
- `unigateway-protocol`
   - 承接 OpenAI / Anthropic 请求解析与中立 HTTP 响应格式化，不再依赖 axum。
- `unigateway-host`
   - 承接 host contract、host bridge、targeting 与面向 core 的执行封装。
- `unigateway-cli`
   - 承接 clap 命令树、guide/setup 流程、render/process 逻辑和 CLI 相关测试。

这一步之后，root crate 不再承担 CLI 大量执行细节，也不再把协议转换或配置内部逻辑硬塞在 `src/` 目录里。

**admin 子域收口**

已经完成的 admin 方向重构包括：

- admin HTTP handler 收口到 `src/admin/`
- `queue_metrics` 移到 admin 子域
- MCP 管理入口移到 `src/admin/mcp.rs`
- `server.rs` 改为显式挂载 `crate::admin::router()`
- admin handler 使用独立 `AdminState`

这一步的结果是：admin CRUD、指标和 MCP 管理入口不再散落在 root 目录，也不再直接共享运行期的全量 `AppState`。

**gateway 请求路径收口**

已经完成的 gateway 请求路径重构包括：

- 引入 `GatewayRequestState`
- `src/gateway.rs` 暴露独立 router
- `src/middleware.rs` 改为依赖 `GatewayRequestState`
- `src/gateway/support/*` 全部改为依赖 `GatewayRequestState`
- `src/host_adapter/app_host.rs` 的 host trait 实现切到 `GatewayRequestState`

这一步之后，`/v1/chat/completions`、`/v1/responses`、`/v1/embeddings`、`/v1/messages` 不再直接绑定 `Arc<AppState>`。

需要注意的是，这里完成的是 **state 收口**，不是 `gateway/support/*` 的完全消失。`support/*` 仍然存在，且仍按 OpenAI chat / responses / embeddings 与 Anthropic messages 这些协议家族拆出 wrapper；它已经不再共享宽状态，但仍然是后续继续收束 dispatch API 的主要残留体量。

**system surface 收口**

本轮最后完成的收口是 system surface：

- 引入 `SystemState`
- `src/system.rs` 改为暴露独立 router
- `/health`、`/metrics`、`/v1/models` 改为挂载在 `SystemState`

这一步完成后，root crate 中运行期 HTTP handler 已不再直接依赖 `Arc<AppState>`。

**`GatewayState` 物理拆薄**

`GatewayState` 已从单体大状态开始拆成组合子状态：

- `ConfigStore`
   - 负责配置文件内容、dirty 状态、sync notifier。
- `RuntimeRateLimiter`
   - 负责 per-key qps / concurrency / queue 运行态。

同时，root crate 已不再直接摸 `GatewayState` 的内部字段，而是通过方法访问最小接口。

**root config shim 现状**

虽然 `unigateway-config` 已经完成物理抽取，但根 crate 里仍保留一个极薄的 `src/config.rs` 兼容层，目前只是对 `unigateway_config::*` 的 re-export。

这说明 root crate 已经不再真正拥有 config 模块实现，但还没有彻底删除旧入口路径。

### 0.2 当前结构结论

当前 root crate 更接近真正的产品壳：

- `AppState`
   - 主要负责启动期装配、持有共享 engine/gateway handle、触发 core sync。
   - 同时也是 `SystemState` / `GatewayRequestState` / `AdminState` 的唯一构造入口，以及当前 admin 测试里最常见的 fixture 入口。
- `SystemState`
   - 负责 `/health`、`/metrics`、`/v1/models`。
- `GatewayRequestState`
   - 负责 `/v1/*` gateway 请求路径所需的鉴权、env fallback 配置和 engine 访问。
- `AdminState`
   - 负责 `/api/admin/*`、`/v1/admin/queue_metrics` 和 admin 相关依赖。

也就是说，当前 HTTP 面已经是三套路由、三种窄 state：

- system surface -> `SystemState`
- gateway surface -> `GatewayRequestState`
- admin surface -> `AdminState`

`AppState` 仍然存在，但它更像 assembly state + test fixture，而不是所有运行期 handler 的共享超集上下文。

### 0.3 验证状态

本轮重构完成后，已经通过以下全量验证：

```bash
cargo fmt --all
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

验证结果为全绿。

### 0.4 当前仍保留的结构债

- `AppState` 仍承担启动期装配与 sync 入口，尚未完全退化成更小的 assembly object。
- `AppState` 仍是各窄 state 的 fan-out 构造入口；如果继续瘦身，下一步要处理的是 assembly/test fixture 边界，而不是运行期 handler 依赖。
- admin router 和 gateway router 仍运行在同一 axum app / 同一进程装配单元里。
- `GatewayState` 虽已拆成组合子状态，但 config facade 仍然偏厚。
- host crate 虽已拥有更中立的 dispatch API，但 env provider 家族仍以固定枚举形式存在于 `unigateway-host::env` 子模块，扩一种 env-only provider 仍需改代码。
- `ProtocolHttpResponse` 已完成更名，但对外 contract 的真正稳定度还需要通过一轮 embedder 实践验证。
- `Endpoint` 上新增的 `provider_name` / `source_endpoint_id` / `provider_family` 已被 hint 匹配逻辑使用，但字段语义和 embedder 侧填充约定仍缺文档。
- 根 crate 仍保留 `src/config.rs` 兼容 shim；如果要继续缩薄入口，需要最终让 root 直接依赖 `unigateway_config` 而不是保留 re-export。

## 1. 三层现实定位

### 1.1 `unigateway-core` — 纯执行引擎

词表：pool / endpoint / driver / `ExecutionTarget` / `ProxySession`。

- 输入：协议无关的 `ProxyChatRequest` / `ProxyResponsesRequest` / `ProxyEmbeddingsRequest` + `ExecutionTarget`。
- 输出：规范化的 `ProxySession` / `CompletedResponse`。
- 不依赖 axum、配置文件、HTTP 路由。

这一层抽象相对干净，是最接近"可独立复用"的部分。

### 1.2 `unigateway-host` — 协议适配 + Host 抽象

当前 `unigateway-host` 实际同时承担三件事：

- **Host 抽象**：`HostContext` + `PoolHost`，以及更中立的 `HostDispatchTarget` / `HostProtocol`；env-only 能力现在单独收在 `EnvProvider` / `EnvPoolHost` 子模块里。
- **协议翻译**：已独立到 `unigateway-protocol`，负责 JSON payload ↔ `Proxy*Request`、`ProxySession` / completed response ↔ OpenAI/Anthropic 兼容响应与 SSE。
- **env-fallback 的临时 pool 投影** + 中立 HTTP 响应封装。

### 1.3 `unigateway-config` — 配置持久化 + admin mutation + core 投影

当前 `unigateway-config` 已经独立成 workspace crate，主要承接：

- **配置文件模型与持久化**：`GatewayConfigFile`、`GatewayState::load/persist`。
- **admin / CLI 共享的配置读写接口**：`create_service` / `create_provider` / `create_api_key` / `list_mode_views` 等。
- **config → core pool 投影**：`core_sync::sync_core_pools`。
- **配置域 routing helper**：`resolve_upstream` / `normalize_base_url`。

这一刀让 `GatewayState` 的所有权边界第一次从根 crate 里真正切了出来，但它内部仍然还是一个较重的聚合对象。

### 1.4 根 crate `unigateway` — 装配 binary

在 `unigateway-config` 抽出之后，根 crate 明显变薄，但依然是**发行版 binary 的装配 crate**：

| 子域 | 关键文件 | 行数量级 | 占比 |
| --- | --- | --- | --- |
| CLI / setup / guide / renders | `unigateway-cli/*` + `main.rs` glue | 仍是最大头 | — |
| 产品域（CRUD handler + 路由辅助） | `admin/*` / `routing.rs` / `dto.rs` / `mcp.rs` | 中等 | — |
| HTTP 网关壳 | `server.rs` / `middleware.rs` / `gateway.rs` / `gateway/support/*` | 中等 | `gateway/support/*` 仍保留按协议家族拆分的 wrapper，是 H5 对应的残留体量 |
| 其他（telemetry / system / upgrade / sdk / host adapter / types） | — | 中等 | — |

也就是说，目前 `unigateway` 的"产品壳"定位依然是"**发行版 binary 的装配 crate**"：承担 CLI、HTTP、admin handler、auth/quota/queue、引擎装配、env 兜底等多件事，但运行期 HTTP surface 现在已经分别收口到 `SystemState` / `GatewayRequestState` / `AdminState`，`AppState` 更多退回为启动与同步装配节点。

## 2. 当前抽象的裂缝

### 2.1 host 层的裂缝

**H1. host 主 contract 已经把 env fallback 退回成可选能力，但 env provider 家族仍是显式产品意见。**
`PoolHost` 现在只要求 service -> pool 的快路径查找；env fallback 已拆到独立的 `EnvPoolHost` / `EnvProvider` 子模块，不再强制所有 embedder 实现。这解决了“无 env fallback 的 embedder 也得写桩代码”的主问题。

当前剩余问题主要是：env provider 家族仍以固定枚举形式出现在 `unigateway-host::env` 中，root shell 也仍围绕 OpenAI / Anthropic 这两个家族做显式分派。它已经不再污染主 host contract，但还没有进化到完全可插拔的 provider-family registry。

**H2. 对外执行 API 已收束成更中立的 dispatch contract。**
`unigateway-host` 现在公开的是 `dispatch_request(host, target, protocol, hint, request)` 这一层，而不是四个按协议家族命名的 wrapper。service lookup 与显式 pool 都通过 `HostDispatchTarget` 进入同一条路径，OpenAI/Anthropic chat、responses、embeddings 则通过 `HostProtocol` + `HostRequest` 区分。

这一步基本消除了“协议 wrapper 直接定型在 SDK 面上”的问题。剩余课题不再是“是否继续消灭 wrapper”，而是未来是否要把 `HostProtocol` 继续抽象成更开放的 trait/object 机制，让新协议类型不需要修改宿主 crate 的枚举定义。

**H3. hint 过滤虽然已脱离 metadata 约定，但还没完全下沉成更强的一等抽象。**
`endpoint_matches_hint` 现在已经优先读 `Endpoint` 上的一等字段（`provider_family` / `provider_name` / `source_endpoint_id`），不再依赖 metadata 字符串 key；但 `build_execution_target` / `build_openai_compatible_target` 仍是 host 层自由函数，还没有进一步收束成更中立的 target builder / execution plan API。

**H4. 旧 routing host 裂缝已经清理，但 hint 入口仍留在产品壳。**
`RoutingHost::resolve_providers` 与 `ResolvedProvider` 已删除，执行路径现在彻底统一成 `pool_for_service` / `env_pool` → targeting → core engine。剩余问题是 provider hint 的抽取仍发生在产品壳里，而真正的匹配规则在 host/core 边界内。

**H5. 产品壳里仍按协议家族挂路由，但 host 对外 contract 已不再被协议 wrapper 绑死。**
root crate 的 `execution_flow.rs` 与 `gateway/support/mod.rs` 仍然按 OpenAI chat / responses / embeddings 和 Anthropic messages 这些 HTTP surface 组织代码；这属于产品壳路由面仍带协议感知的自然结果。

但和前一阶段不同，这种协议感知已经不再外溢到 `unigateway-host` 的公开执行入口上。换句话说，H5 现在可以重新回归为产品壳内部债，而不是 SDK contract 债。后续若继续做收口，重点应该是把 root crate 的请求准备 / 执行 / 响应链进一步压缩，而不是继续重构 host 主 API。

### 2.2 根 crate 的裂缝

**A1. `AppState` 是个 god object。**
它仍然同时装 `AppConfig` / `Arc<GatewayState>` / `Arc<UniGatewayEngine>`，但现在 system handler 已切到 `SystemState`，admin handler 已切到 `AdminState`，gateway 请求 handler 也已切到 `GatewayRequestState`，host trait 也不再由 `AppState` 直接实现。这说明 `AppState` 的 god object 风险已经明显下降；当前更准确的风险形态不是“所有 handler 都能摸到一切”，而是 **assembly fan-out**：`AppState` 仍是各窄 state 的唯一构造入口，也是测试里最常见的 fixture 入口，启动与测试边界还没有完全分开。

**A2. `GatewayState` 也是 god object。**
一把 `RwLock<GatewayConfig>` 守文件内容，一把 `Mutex<HashMap<String, RuntimeRateState>>` 管 per-key 配额，加 core_sync notifier；admin / select / store / core_sync 全都 `impl GatewayState`（单 `unigateway-config/src/admin.rs` 就是一大坨这种扩展）。

> 真实风险不在单一"锁争用"，而是多把锁（`inner` RwLock 管配置、`api_key_runtime` Mutex 管配额）+ 多个职责（配置文件、鉴权查询、配额状态、sync 触发）糅合在一个类型上。部分 admin 路径需要同时修改配置和配额状态，形成了不容易推理的"状态变更传播"。

**A3. CLI 子域体量失衡。**
这部分问题已经基本从根 crate 移走了，但 `unigateway-cli` 里仍有几块很重的渲染/交互代码（例如 `guide.rs`、`setup.rs`、`render/integrations.rs`），后续还可以继续做模块内聚和职责收束。

**A4. `main.rs` 631 行几乎都是 clap 树。**
真正的 `main` 逻辑只有十几行，剩下全是 `#[derive(Subcommand)]` 和命令 dispatch。

**A5. admin API 与对客户端暴露的 `v1` API 只有部分边界。**
`/api/admin/*` 现在已经通过 `src/admin/mod.rs` 聚合，并由 `server.rs` 显式 `merge(crate::admin::router().with_state(admin_state))`；`service` / `provider` / `api_key` CRUD 与 `queue_metrics` 也都收进 `src/admin/` 子模块，admin handler 已改用专门的 `AdminState`，不再直接绑定 `Arc<AppState>`。这一步解决了 root 目录平铺、route 注册分散，以及 admin/shared product state 绑得过紧的问题。但 admin 与 `POST /v1/chat/completions` / `POST /v1/responses` / `POST /v1/embeddings` / `POST /v1/messages` 仍注册在同一 axum app 与同一进程装配单元里。未来若要分离端口、加更独立的 admin auth/middleware、或禁用管理面，仍需要继续把 admin 装配与产品 HTTP 面彻底拆开。

**A6. env 兜底的 provider 列表硬编码进 `AppConfig`。**
新增一种 env-only provider（如 gemini）仍要同时改 `AppConfig`、`GatewayRequestState`、`HostEnvProvider`、`PoolHost::env_pool` 的 provider 分派，以及对外暴露该 provider 的 HTTP surface。虽然比旧的 `HostConfig` / `RuntimeConfigHost` / `build_env_*_pool` 多处散落实现少了不少，但 provider 家族列表仍是硬编码的。

**A7. provider hint 的边界还没有完全收束。**
旧的 `resolve_providers` 双轨已经删掉，但 `target_provider_hint` 仍位于产品壳，而真正的 hint 解释规则在 `unigateway-host/src/core/targeting.rs`。也就是说，"hint 从哪里来" 与 "hint 如何匹配 endpoint" 仍跨着 product/host 边界。

**A8. config crate 已独立，但职责仍偏厚。**
`unigateway-config` 现在已经拥有更清晰的边界，但它内部仍把 schema、持久化、admin mutation、限流运行态、core sync 放在同一个 crate / 同一个主状态对象里；这只是把边界从根 crate 切出来，还不是完成内聚度治理。

## 3. 重构建议（按 ROI 排序）

### 3.1 先做：收益大、风险小（第一轮）

1. **在已有 `env_pool` 合同的前提下，把「两套路由」收束成单一路径。**
   **已完成。** `env_pool` 与 `pool_for_service` 现在统一通过 dispatch target 解析进入 `dispatch_request`；env 请求不再有独立的 host API 面。

2. **消灭 host 层剩余 API 重复。**
   **已完成。** host 对外执行入口已经收束成 `dispatch_request`；HTTP error/response 组装也已留在 root crate 的 `response_flow.rs`。剩余重复主要是产品壳仍按 4 个 HTTP surface 组织路由，而不是 host public API 仍然分叉。

3. **把 hint / kind 过滤下沉到 core**。
   引入 `ExecutionTarget::Plan` 的 builder（按 family / kind / hint 过滤），`Endpoint` 加 `family: Option<String>` / `tags: Vec<String>` 一等字段，删掉对 metadata 字符串约定的依赖。这样 core 层可以独立校验 hint 匹配，不依赖外部约定。

4. **从 `HostConfig` 拿掉具体 provider 字段**。
   **已完成。** `RuntimeConfig` / `RuntimeConfigHost` 已删除，env fallback 完全走 `PoolHost::env_pool` 与 embedder 侧的 provider 分派。

5. **评估并可能删掉 `RoutingHost`**。
   **已完成。** `RoutingHost` 与 `ResolvedProvider` 已从 host contract 中移除，真正的执行路径已经是 pool_for_service / env_pool → targeting（hint 匹配） → core engine。

### 3.2 然后做：结构性的分层调整（第二轮）

6. **拆 `unigateway-cli` crate**。
   **已完成第四刀。** `unigateway-cli` 现在不仅承接 clap 命令树、子命令参数类型与 `GuideCommand`，也承接了 CLI 执行逻辑、setup flow 和原先挂在 root shim 里的 CLI 回归测试：`diagnostics`、`guide`、`modes`、`process`、`render/*`、`setup`、`tests` 都已在独立 crate 内收口。根 crate 已删除 `src/cli.rs`，`src/main.rs` 只保留顶层 dispatch 与 binary startup glue。

   这里的“已完成”仅指 CLI 抽取这条子任务，不指整个第二轮或整份基线中的重构任务都已完成。

   剩余工作：
   - 让根 crate 进一步收缩到 tokio runtime 装配 + dispatch glue

7. **拆 `unigateway-config` crate**。
   **已完成第一刀。** `unigateway-config` 已成为独立 workspace crate，原 `src/config.rs` + `src/config/*` 已迁入该 crate，`GatewayState` 也已经成为公共 handle；根 crate 当前仅保留薄 re-export 兼容层。`resolve_upstream` / `normalize_base_url` 也已迁入 `unigateway-config::routing`。剩余工作不再是“是否拆 crate”，而是继续切薄 `GatewayState` 本身，并决定哪些 admin / registry 逻辑继续下沉到该 crate。

8. **把协议翻译拆成独立 crate `unigateway-protocol`**。
   **已完成。** 现在由独立的 `unigateway-protocol` crate 承担 payload ↔ `Proxy*Request`、**ProxySession ↔ 中立响应类型**（不是 `axum::Response`）的转换，且**不依赖 axum**。具体地：
   - 请求解析位于 `unigateway-protocol/src/requests.rs`
   - 响应格式化与 `ProtocolHttpResponse` 位于 `unigateway-protocol/src/responses.rs` / `src/http_response.rs`
   - `unigateway-host` 与根 crate 都直接依赖该 crate，而不再通过 host 内部模块转发协议逻辑
   - `unigateway-core` 的 `proxy_*` 继续只返回 `ProxySession` 等中立结果，**不**与 axum 耦合
   这一步完成后，`unigateway-host` 的边界已经明显收缩为 host contract + dispatch。

9. **完成 `unigateway-runtime` → `unigateway-host` 物理重命名**。
   **已完成。** crate 名、目录名、根 crate 依赖名，以及 `HostContext` / `PoolHost` / `EnvPoolHost` / `EnvProvider` 等公开符号都已经收敛到 host 命名。
   这一步带来的直接收益：
   - crate 语义不再与 async executor 的 “runtime” 概念冲突
   - `unigateway-host` 与 `unigateway-protocol` / `unigateway-core` 的职责边界更直观
   - 根 crate 内部的 `src/host_adapter/` 也与当前 contract 命名一致

### 3.3 再做：解耦 god object（第三轮）

10. **切开 `AppState` 与 `GatewayState`**。
   **已完成第五步。** root product shell 已不再直接访问 `GatewayState::inner` / `GatewayState::api_key_runtime`，并且 `GatewayState` 本体也已经从单体字段改成组合子状态：
      - `middleware.rs` 的 per-key qps / concurrency 限流与释放逻辑已经收进 `unigateway-config/src/runtime.rs`，通过 `GatewayState::{acquire_runtime_limit, release_api_key_inflight, queue_metrics_snapshot}` 暴露最小接口。
      - `mcp.rs` 不再直接翻 `gateway.inner`，而是通过 `list_services_with_routing()` 与 `config_snapshot()` 读取配置视图。
      - system handler 已改用 `SystemState`，admin handler 已改用 `AdminState`，gateway 请求 handler 已改用 `GatewayRequestState`；`host_adapter` 的 host trait 也随之绑定到更窄的 gateway 请求 state，而不是 `AppState`。
   - `unigateway-config/src/lib.rs` 中的 `GatewayState` 已开始物理拆分为 `ConfigStore`（文件状态 + dirty + sync notifier）与 `RuntimeRateLimiter`（per-key qps / concurrency / queue）。
   剩余目标仍然是：
      - `AppState` 进一步只保留启动、同步、组装和必要的测试 fixture 角色；如果继续下沉，下一步更可能是把 sync / process lifecycle 与 test fixture 入口切成更明确的装配对象，而不是继续让 handler 依赖 `Arc<AppState>`。
   - `GatewayState` 继续下沉或最终退化为 façade handle；如果继续拆，下一候选是 `ServiceRouter`（rr 计数器 / 选择态）。
   - 三者独立加锁，彻底消掉"写配置阻塞配额"与"配置 façade 过重"的路径耦合。

11. **Admin router 独立组装**。
   **已完成第三步。** `server.rs` 现在已经显式 `merge(crate::admin::router().with_state(admin_state))`，`service.rs` / `provider.rs` / `api_key.rs` / `queue_metrics` / `mcp.rs` 都已收进 `src/admin/` 子模块；admin handler 也改为依赖专门的 `AdminState`，MCP 管理入口也已经落到同一 admin 子域。剩余工作主要是决定是否把 admin router / MCP 再从共享 axum app 与共享 binary 装配中拆成独立端口或独立装配单元。未来想给 admin 绑单独端口或加专属 middleware，还需要继续完成最后这半步。

12. **产品侧的 "provider hint" 统一到 host/core**。
    `src/routing.rs` 的匹配逻辑删掉，只保留 header / payload 提取；匹配交给 host 层的 `ExecutionPlan` builder 读 endpoint 的一等字段。

## 4. 命名：为什么从 `runtime` 改名为 `host`

当前 `unigateway-runtime` 的命名是有问题的：

- **真正意义上的 "runtime" 是 `UniGatewayEngine`**——拥有 pool 注册表、驱动调度、重试、流式生命周期，那才是执行时态的运行环境。`unigateway-runtime` 里没有任何"运行时"语义，它做的是协议适配器 + 宿主接缝。
- Rust 生态里 "runtime" 几乎被 `tokio` / `async-std` 那类 async executor 独占。读 `use unigateway_runtime::...` 的人第一反应是"异步调度层"，和它的真实职责冲突。
- 随着 [§3.2 的 crate 拆分](#32-然后做结构性的分层调整第二轮) 把协议翻译抽成独立 `unigateway-protocol` crate，残体就只剩"host 抽象 + env→pool 投影 + dispatch"，这恰好是 `HostContext` / `*Host` trait 体系的本体。

**改名应该放在第一轮修复 contract 和第二轮抽取 protocol 之后的收尾**，而不是和第二轮同一轮执行。原因是：当前 API 还带着 provider-specific env 字段（`HostConfig` 的 openai_/anthropic_*），这时改名只是换门牌，屋里结构没变。等到第一轮真正统一了 env pool 的来源渠道、第二轮明确了 protocol 边界，改名才是有意义的确认。

> 保留可能：如果最终评估 `unigateway-host` 体量太薄且没有第二个 embedder，可以直接并回根 crate 成为 `src/host/` 模块，独立 crate 的唯一收益就是强制边界。保留与否在执行阶段再决定。

## 5. 目标形态速写

如果以上步骤走完，crate 拓扑大致是：

```text
unigateway-core       纯引擎：pool / driver / retry / streaming，无协议绑定
  ↓ 对外：UniGatewayEngine + Proxy*Request / ProxySession

unigateway-protocol   纯翻译：JSON ↔ Proxy*Request、ProxySession ↔ 中立响应，无 axum
  ↓ 对外：无状态编解码函数

unigateway-host       Host 抽象 + 执行分派：HostContext、env→pool 投影、dispatch(protocol, auth, hint, request)
  ↓ 对外：中立的响应中间态（enum 或 trait 模板）

unigateway-config     配置 / persist / admin / core_sync
unigateway-cli        CLI 命令面、CLI 执行逻辑、guide、integrations renders

unigateway（根 bin）   HTTP server + middleware + 装配（含 axum 封装）
```

根 crate `unigateway` 最终只做一件事：**把 config、host、core 组装起来启动一个 HTTP 服务**，代码量在几百行级别。

换一个视角：现在 `ug` 的定位更像"**一个装了 CLI、配置、admin、HTTP server 的全家桶**"；它想成为的定位应该是"**一个最小装配 binary**"。中间差的那层，就是第 3 节列出的几刀。

## 6. 执行注意事项

- 每一刀都应该是**单独 PR**、保持可编译可发布，不要把多轮混在一起。
- **第一轮**的重点是**在已有 `env_pool` 的基础上收束对外的调用面**（合并 `try_*_via_core` / `try_*_via_env` 与 `execution_flow` 的 auth 分叉），而不是「从零让 env 走 PoolHost」——后者在代码里**已经**通过 `PoolHost::env_pool` 落地。步骤 2 的删 API 应在步骤 1 的「单一路径」能覆盖全部场景后执行。
- **第二轮**里 protocol crate 抽取的第一道门槛已经完成：`axum::Response` 已经从 runtime 业务路径里请出去，host 只返回中立 `RuntimeHttpResponse`，core 继续返回 `ProxySession` 等。后续抽 crate 时要避免只是“移动目录”，而要顺手把协议格式化职责与 host dispatch 职责拆开。
- 改名（步骤 9）在 protocol 与第一轮路由收束**之后**单独做一轻量 PR 即可，逻辑应等同于移动与重命名，方便 bisect。
- 拆 `unigateway-config` 之前先把 `GatewayState` 的内部职责理清（配置、配额、轮询、sync），否则新 crate 一样会继承 god object。可和第一轮的 `execution_flow`/pool 面梳理并行，但不要堆到第三轮才动。
- 本轮已经删除 `RuntimeConfig` 等 provider-specific host 配置字段。如果有外部 embedder 依赖，后续 release notes 需要明确标注这次 contract 收敛。
- 每轮结束都更新 [`memory.md`](./memory.md) 中的"核心心智模型"章节，避免文档与代码脱节。跟踪哪些 trait 已删除、哪些 API 合并了、哪些边界明确了。
