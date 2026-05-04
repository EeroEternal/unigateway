# 嵌入栈与 SDK 门面：状态同步

本文记录 UniGateway 当前的嵌入栈形态、`unigateway-sdk` 的实际定位，以及已经完成和仍待继续收口的事项。它是 [`refactor-baseline.md`](./refactor-baseline.md) 的补充：baseline 偏分层债与架构裂缝，本文偏嵌入模型、门面 crate、版本约定与发布状态。

## 1. 当前嵌入栈

对把 UniGateway 当库使用的应用而言，当前稳定的嵌入栈是三层：

| 层级 | Crate | 当前职责 |
| --- | --- | --- |
| 执行引擎 | `unigateway-core` | `UniGatewayEngine`、pool、driver、重试、流式、`GatewayHooks`；不依赖 Axum，不绑定 ug 的 TOML。 |
| 协议与响应形态 | `unigateway-protocol` | JSON ↔ `Proxy*Request`；`ProxySession` ↔ `ProtocolHttpResponse`；不执行上游调用。 |
| 宿主接缝与分派 | `unigateway-host` | `HostContext`、`PoolHost` / `EnvPoolHost`、dispatch、targeting、调用 `proxy_*` 并衔接 protocol 渲染。 |

本仓库**不再提供**根 binary 产品壳或 `unigateway-cli`。嵌入方通常从 **`unigateway-sdk`** 起步；若需要持久化配置与 admin 语义，再按需依赖 **`unigateway-config`**。HTTP 路由、鉴权、admin、MCP、进程生命周期都属于**宿主应用**（例如你们自研的管理网关），不属于 SDK 的默认依赖面。

宿主应用与 `unigateway-sdk` 的关系是**消费者**：在自身进程里装配 `GatewayState` / core 同步 / `HostContext`，把 `HostDispatchOutcome` 映射到自己的 HTTP 栈即可。

## 2. `unigateway-sdk` 的当前状态

`unigateway-sdk` 已经实现并发布；与 workspace 其它成员对齐的版本线见根 `Cargo.toml` 与 crates.io。

当前定位很明确：它是**薄门面**，不是新的底层抽象层。

它现在只做三件事：

- 通过 feature 编排 `unigateway-core`、`unigateway-protocol`、`unigateway-host`。
- 通过命名空间 re-export 暴露 `unigateway_sdk::core`、`unigateway_sdk::protocol`、`unigateway_sdk::host`。
- 在 crate README 与仓库文档中集中声明版本共升规则和嵌入入口。

它现在**刻意不做**的事：

- 不重新定义状态模型。
- 不新增执行 helper、配置装配层或 HTTP middleware。
- 不把所有底层类型扁平化 re-export 到 crate 根。
- 不在门面里解决 `HostProtocol` 可扩展性这类 host 自身演进问题。

也就是说，`unigateway-sdk` 解决的是**依赖入口统一**、**版本边界统一**、**文档入口统一**，而不是再包一层新的 SDK 语义。

## 3. `unigateway-sdk` 的实际导出面

当前 feature 布局如下：

- `default`：默认即完整嵌入栈，实际落在 `host` 这条 canonical feature 上。
- `core`：仅暴露 `unigateway-core`。
- `protocol`：暴露 `unigateway-core` + `unigateway-protocol`。
- `host`：canonical 的完整 host-facing 栈；当前依赖顺序为 `host -> protocol -> core`。
- `embed`：`1.x` 兼容别名，等价于 `host`，保留是为了不打断已发布依赖写法。
- `testing`：在 `host` 之上继续转发 `unigateway-host/testing`，方便 facade-only embedder 复用测试夹具。

当前推荐的依赖入口是：

```toml
[dependencies]
unigateway-sdk = "1.10"
```

当需要更细粒度控制时，嵌入方仍然可以直接依赖三层 crate；但那属于显式选择，不再是默认推荐路径。若混用底层 crate，应保持与 `unigateway-sdk` 同一 release line。

## 4. 已完成的 API 收口

以下收口已经落地，并已反映在当前代码与文档中：

- **env 与主 host contract 解耦**：`PoolHost` 只负责 `pool_for_service`；env fallback 单独进入 `unigateway_host::env::EnvPoolHost`，默认 `Ok(PoolLookupOutcome::NotFound)`。
- **`EngineHost` 删除**：`HostContext::from_parts(engine, pool_host)` 直接持有 `&UniGatewayEngine`。
- **中立响应类型更名**：`unigateway-protocol` 对外统一使用 `ProtocolHttpResponse`，不再暴露旧的 `Runtime*` 命名。
- **统一 dispatch 入口**：host 对外主路径已收束为 `dispatch_request` + `HostDispatchTarget` + `HostProtocol` + `HostRequest`。
- **host 错误已类型化**：`dispatch_request` 已返回 `HostError`，embedder 可以稳定区分 dispatch 错配、pool lookup、targeting 与 core execution 失败，再由宿主应用完成 HTTP 适配。
- **host execution 失败继续细化**：`HostError` 不再只包一层 `GatewayError`，而是把 upstream http、transport、stream abort、invalid request、all attempts failed 等执行失败展开成 host-facing 变体。
- **pool lookup 错误也开始脱离 `anyhow`**：host contract 现在区分 `PoolLookupOutcome` 与 `PoolLookupError`，embedder 可以把 timeout / unavailable 这类 lookup 失败作为类型化错误返回，而不是只剩字符串。
- **dispatch target 语义更明确**：宿主侧已不再用裸 `Option<HostDispatchTarget>` 表达 env fallback 结果，而是显式区分 `DispatchTarget` 与 `PoolNotFound`。
- **pool lookup outcome 已显式化**：`PoolHost` / `EnvPoolHost` 现在对外返回 `PoolLookupOutcome`，不再把“查到 pool”和“没有可用 pool”混在 `Option` 语义里。
- **Anthropic 请求 model alias 收口**：通过 `ANTHROPIC_REQUESTED_MODEL_ALIAS_KEY` 贯穿请求解析、streaming metadata 与响应渲染，不再额外传 `requested_model` 参数。
- **SDK 门面已发布**：`unigateway-sdk` 已加入 workspace、补齐 README / guide / changelog，并已发布到 crates.io。
- **embedder 测试夹具已公开**：`unigateway-host::testing` 已通过 feature gate 对外开放，`unigateway-sdk` 也同步转发该 feature。
- **SDK feature 组合已入 CI**：workflow 现在会对 `core` / `protocol` / `host` / `embed` / `testing` 组合做校验，并补一轮 feature 级 `cargo test`。

## 5. 当前剩余事项

当前还没有完成的，不再是“是否应该有 SDK 门面”，而是门面之外的少量 contract 打磨：

- **`embed` 兼容别名仍会继续存在一段时间**：主语义已经收束到 `host`，但为了 1.x 平滑兼容，短期内不会直接删除 `embed`。
- **`HostProtocol` 只是 `#[non_exhaustive]`，仍不是插件式协议注册**：新增协议族仍需要修改 `unigateway-host`。
- **dispatch 仍是运行时配对**：虽然现在是 typed `HostError`，但 `HostProtocol` 与 `HostRequest` 的错配仍不是编译期禁止。
- **env 符号仍显式公开**：`EnvProvider` / `EnvPoolHost` 已经不污染主 contract，但是否还要进一步缩 visibility 或通过门面再收口，仍可评估。
- **HTTP 适配完全在宿主侧**：根产品 crate 已删除；host 只返回结构化结果与中立的 `ProtocolHttpResponse`，由嵌入方映射到自己的 HTTP 框架。

这些剩余项的重点已经从“crate 边界拆分”转向“嵌入 contract 继续抛光”。

## 6. 文档与版本状态

当前已同步到位的文档包括：

- [`embed.md`](../guide/embed.md)：嵌入指南已改为以 `unigateway-sdk` 为第一推荐路径。
- [`refactor-baseline.md`](./refactor-baseline.md)：已同步 host / protocol contract 收口事实。
- 仓库 [`README.md`](../../README.md)：已补充 embedder 入口和版本共升说明。
- `CHANGELOG.md`：已补充 `unigateway-sdk` 发布条目。

当前版本约定：默认推荐直接依赖 `unigateway-sdk`；若混用 `unigateway-core`、`unigateway-protocol`、`unigateway-host`，应保持同一 release line（例如与 `1.10.x` 对齐）。

## 7. 与 `refactor-baseline.md` 的分工

| 文档 | 当前侧重 |
| --- | --- |
| [`refactor-baseline.md`](./refactor-baseline.md) | 分层债、结构裂缝、后续仍值得继续拆的部分。 |
| **本文** | 嵌入栈现状、`unigateway-sdk` 定位、发布状态、版本与文档同步结果。 |

维护者：若嵌入模型、crate 拓扑或门面定位发生变化，应优先更新本文，并同步检查 [`memory.md`](./memory.md) 与 [`embed.md`](../guide/embed.md) 是否仍与当前事实一致。

## 8. 给 ParaRouter 的审计摘要

如果要从 embedder / contract 审计视角快速复核，这一轮优化可以概括成三条主线：

- **SDK 入口语义更清晰**：`unigateway-sdk` 的默认完整栈现在以 `host` 作为 canonical feature；`embed` 继续保留，但只作为 `1.x` 兼容别名。
- **host 错误面更可判别**：host dispatch 不再主要依赖字符串化的 `anyhow` 传播，而是通过 `HostError` 区分 dispatch 错配、pool lookup、targeting、core execution 等失败类型。
- **pool lookup contract 更显式**：`PoolHost` / `EnvPoolHost` 不再返回 `Result<Option<ProviderPool>>`，而是返回 `PoolLookupOutcome`，把“lookup failure”和“pool 不存在”从语义上拆开。

从代码收口角度，这一轮已经完成的核心事项是：

- `unigateway-sdk` feature 语义从“`default` / `embed` 双主名”收束为“`host` 是 canonical full-stack feature，`embed` 是兼容别名”。
- `unigateway-host` 对外错误面已经类型化，embedder 不需要再靠字符串匹配判断 dispatch 失败类型，并且常见 execution failure 已经细化到可直接消费的枚举变体。
- `PoolHost` / `EnvPoolHost` 的错误路径也不再停在裸 `anyhow`，而是进入显式的 `PoolLookupError` 类型。
- 产品壳内部的 env fallback dispatch 也已经跟随 `PoolLookupOutcome` 对齐，不再把 `None` 当隐式控制流。
- `unigateway-host::testing` 已对外公开，embedder 可以复用 `MockHost` / `build_context` 做接入测试。
- README、embed guide、dev memory、SDK 状态文档、CI feature 检查都已经同步到新的事实。

从兼容策略角度，需要明确告诉审计方的点有两个：

- **保守兼容的部分**：`embed` feature 没有删除，只是降级为兼容别名；默认依赖路径仍然保持简单的 `unigateway-sdk = "1.10"`（或与你锁定的 minor 对齐）。
- **真实 public API 变化的部分**：`PoolHost` / `EnvPoolHost` 的返回签名从 `Result<Option<ProviderPool>>` 变成了 `Result<PoolLookupOutcome>`。这使 contract 更清晰，但也意味着实现这些 trait 的 embedder 代码需要跟着调整。
- **semver 处理方式**：公共 contract 的收紧应在 changelog 中单列 `Breaking Changes`，避免 patch 升级 silently 打穿下游实现。

因此，建议 ParaRouter 重点复核以下问题：

- `PoolLookupOutcome` 这次是否应视为合理的 contract tightening，还是应该进一步改成“新增 method / 新增 trait，旧签名先保留一段时间”。
- `HostError` 当前的层级是否已经足够稳定，还是还需要更细的 env-fallback-not-available / timeout 等专门类型区分。
- `HostProtocol` + `HostRequest` 仍是运行时配对，这一层是否还值得继续推进到编译期约束。
- `EnvProvider` 仍是显式枚举，host contract 虽已收窄，但协议家族扩展性还没有完全打开。

当前这轮修改已经通过以下验证：

- `cargo fmt --all`
- `cargo test --workspace`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo check -p unigateway-sdk`
- `cargo check -p unigateway-sdk --no-default-features --features core`
- `cargo check -p unigateway-sdk --no-default-features --features protocol`
- `cargo check -p unigateway-sdk --no-default-features --features host`
- `cargo check -p unigateway-sdk --no-default-features --features embed`
- `cargo check -p unigateway-sdk --no-default-features --features testing`
