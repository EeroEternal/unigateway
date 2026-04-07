# UniGateway 详细架构分析 — Gemini 任务参考

> 本文档面向执行者（Gemini），提供代码级精确分析。不改代码，只分析。
> 最后更新: 2026-04-07 09:50 (基于 v1.0.0 commit `cf83090`, working tree clean)
> `cargo check` 零 warning, `cargo test --workspace` 18 通过 / 0 失败

---

## 1. 仓库结构概览

```text
unigateway/                           # workspace root + product shell binary (v1.0.0)
├── Cargo.toml                        # ✅ 零 llm-connector 依赖
│                                     # 依赖: axum, clap, unigateway-core, unigateway-runtime
│                                     # ⚠️ llm_providers 0.8.1 (Setup/CLI 用, 非热路径)
│
├── unigateway-core/ (v1.0.0)         # 纯内存执行引擎
│   └── src/ (13 files, ~5900 行)     # ✅ 零外部框架依赖
│
├── unigateway-runtime/ (v1.0.0)      # 可复用运行时层
│   └── src/ (5 files, ~2030 行)      # ✅ 零 llm-connector
│
└── src/ (~7800 行)                   # 产品壳
```

---

## 2. 迁移完成状态

**所有 8 个迁移任务已完成。`llm-connector` 已从代码库彻底移除。**

| Task | 状态 | 解决版本 |
|---|---|---|
| 1. Runtime 移除 llm-connector | ✅ | v0.10.3 |
| 2. 清理 #[path] 重映射 | ✅ | v0.10.3 |
| 3. 事件驱动 Pool 同步 | ✅ | v0.10.3 |
| 4a-d. Fallback + env-key + encoding_format | ✅ | v0.10.3-4 |
| 5. Engine 多尝试循环 | ✅ | v0.10.4 |
| 6. GatewayHooks 接入 | ✅ | v0.10.4 |
| 7. 请求路径 Core-Native 化 | ✅ | `245e095` |
| 8. 移除 legacy_runtime.rs + llm-connector | ✅ | `a402b02` |

---

## 3. 当前请求生命周期

```
JSON payload
  → protocol.rs (JSON → ProxyChatRequest 直达)
  → execution_flow.rs (resolve_core_only_runtime_flow)
  → runtime/core.rs (prepare_core_pool → engine.proxy_chat → 响应整形)
  → engine.rs (多尝试循环 + hooks + backoff)
  → protocol/openai.rs 或 anthropic.rs (driver 调用上游)
  → 响应返回
```

---

## 4. 代码热点 (按行数排序)

| # | 文件 | 行数 | 关注度 |
|---|---|---|---|
| 1 | `unigateway-core/src/engine.rs` | 1852 | ⚠️ 大但功能密集 |
| 2 | `unigateway-runtime/src/core.rs` | 1518 | ⚠️ 可拆分 |
| 3 | `unigateway-core/src/protocol/openai.rs` | 1230 | 正常 |
| 4 | `src/cli/render/integrations.rs` | 765 | CLI 渲染 |
| 5 | `unigateway-core/src/protocol/anthropic.rs` | 638 | 正常 |
| 6 | `src/main.rs` | 631 | CLI 入口 |
| 7 | `src/setup/mod.rs` | 619 | Setup 交互 |
| 8 | `src/cli/guide.rs` | 610 | CLI 向导 |

---

## 5. 优化建议 (按优先级排序)

### P0 — 立即可做 (5 分钟, 无风险)

#### 5.1 删除 `flow.rs` 中的死代码

`resolve_authenticated_runtime_flow` 和 `resolve_env_runtime_flow` **零调用者** — 产品壳已全部切换到 `resolve_core_only_runtime_flow`。

**删除范围**:
- `flow.rs` L21-59: 两个 dead 函数
- `flow.rs` L125-129: `legacy_error_response` 辅助函数
- `flow.rs` L132-137: `upstream_error_response` 辅助函数
- `status.rs` L12-25: `status_for_legacy_error` 函数
- `status.rs` L42-52: 对应的测试

**预计**: `flow.rs` 从 189 行缩减到 ~100 行, `status.rs` 缩减约 20 行。

### P1 — 短期建议 (结构优化)

#### 5.2 拆分 `runtime/core.rs` (1518 行)

当前文件承担三个不同职责:
1. **Pool 准备 + Engine 调用** (L23-195, ~170 行) — 8 个 `try_*_via_core` 公共函数
2. **OpenAI 响应整形 + Streaming** (L197-624, ~430 行) — SSE 适配、chunk 转换
3. **Anthropic 响应整形 + Streaming** (L245-831, ~590 行) — 协议转换、mpsc stream 驱动
4. **Pool 构建 + 工具函数** (L833-1155, ~320 行) — env pool、hint 匹配、base_url 规范化

**建议拆分**:
```
core.rs               → 入口 + pool 准备 (~200 行)
openai_response.rs    → OpenAI 响应整形 + streaming (~450 行)
anthropic_response.rs → Anthropic 响应整形 + streaming (~600 行)
env_pool.rs           → env pool 构建 + 工具函数 (~300 行)
```

#### 5.3 `config/core_sync.rs` 缺少单元测试

404 行代码，0 个 `#[test]`。`build_pool_from_file`、`to_core_strategy`、`to_core_endpoint` 等函数都适合单元测试。

当前测试覆盖情况:
| 模块 | 测试数 | 行数 | 测试密度 |
|---|---|---|---|
| `runtime/core.rs` | 11 | 1518 | 中 |
| `protocol.rs` (产品壳) | 6 | 326 | 高 |
| `core/protocol/openai.rs` | 4 | 1230 | 低 |
| `runtime/flow.rs` | 3 | 189 | 高 |
| `runtime/status.rs` | 2 | 57 | 高 |
| `core/protocol/anthropic.rs` | 1 | 638 | **很低** |
| `config/core_sync.rs` | **0** | 404 | **无** |
| `engine.rs` | **0** | 1852 | **无** |

### P2 — 中期建议 (API 稳定化)

#### 5.4 消除 env-key per-request `upsert_pool`

4 处 `upsert_pool` 仍在 `runtime/core.rs` 的 `try_*_via_env_core` 函数中。

**优化方案**: 在 `server.rs` 启动时，如果环境变量 `OPENAI_API_KEY` / `ANTHROPIC_API_KEY` 已设置，预创建 `__env_openai__` / `__env_anthropic__` pool。`try_*_via_env_core` 改为先检查 pool 是否已存在，存在则直接使用。

**影响**: 消除无 gateway key 请求的 per-request pool 写锁。

#### 5.5 去除 Runtime 的 `axum` 依赖

`core.rs` 和 `flow.rs` 返回 `axum::response::Response`。如果要让 `unigateway-runtime` 能被非 axum 产品复用，需要:
- 将响应整形函数改为返回 `(StatusCode, HeaderMap, Body)` 元组或自定义 `GatewayResponse` 类型
- 在产品壳中包装为 `axum::Response`

这是一个 API 设计决策，需要评估是否真有非 axum 复用场景。

#### 5.6 `llm_providers` 依赖评估

`llm_providers 0.8.1` 仍被 4 个文件引用，但**全部在 CLI/Setup 路径**，不影响请求热路径:
- `src/routing.rs` — `get_endpoint()` 解析 endpoint_id
- `src/config/admin.rs` — `get_endpoint()` 验证 provider 元数据
- `src/setup/prompts.rs` — `list_models_for_endpoint()` Setup 向导
- `src/setup/registry.rs` — `get_providers_data()` 注册表

可以考虑是否要将这些功能内化到 `unigateway-core` 或维持现状。

### P3 — 代码质量

#### 5.7 `routing.rs` 的 `#[allow(dead_code)]`

`unigateway-core/src/routing.rs:10` 的 `ExecutionSnapshot` struct 标记了 `#[allow(dead_code)]`。检查其 `metadata` 字段是否真的需要保留。

#### 5.8 `engine.rs` + `core/protocol/anthropic.rs` 缺乏单元测试

Engine (1852 行) 和 Anthropic driver (638 行) 是代码量最大的两个文件，但 engine.rs 有 0 个测试，anthropic.rs 只有 1 个。建议至少覆盖:
- `should_retry_error` 逻辑
- `apply_retry_backoff` 行为
- `attempt_endpoints` 的 Fallback/Random/RR 排序

---

## 6. 总结: v1.0.0 架构成熟度

| 维度 | 评分 | 说明 |
|---|---|---|
| **层次分离** | ⭐⭐⭐⭐⭐ | core 零外部依赖, runtime 仅依赖 core+axum, 产品壳只依赖两个内部 crate |
| **外部依赖隔离** | ⭐⭐⭐⭐⭐ | llm-connector 完全移除, llm_providers 仅在 CLI 路径 |
| **请求路径简洁性** | ⭐⭐⭐⭐⭐ | JSON → core 类型直达, 零中间转换 |
| **Engine 功能** | ⭐⭐⭐⭐⭐ | 多尝试 + Fallback/RR/Random + Hooks + Timeout + Backoff |
| **配置同步** | ⭐⭐⭐⭐ | 事件驱动同步, env-key 仍 per-request (轻微) |
| **测试覆盖** | ⭐⭐⭐ | 18 个测试, 关键模块有覆盖但 engine/core_sync 缺乏测试 |
| **死代码** | ⭐⭐⭐⭐ | 仅 flow.rs 有少量残留 |
| **文件粒度** | ⭐⭐⭐ | runtime/core.rs 1518 行可拆分 |

**结论**: v1.0.0 的架构已经非常成熟。核心迁移目标全部达成。剩余优化都是打磨级别的工作，不影响功能或生产使用。
