## UniGateway 项目架构与模块分析

### 1. 项目定位与整体结构

**项目定位**：轻量级开源 LLM 网关 + CLI 工具，兼容 OpenAI 与 Anthropic 协议，提供：
- 统一 HTTP 网关接口（`/v1/chat/completions`、`/v1/messages`）
- 多上游 Provider 路由与模型映射
- 网关级 API Key、配额 / QPS / 并发控制
- 基于 `clap` 的场景化 CLI 管理能力（service / provider / api-key / metrics 等）
- （可选）SDK 客户端，方便其他服务以代码方式调用

**核心目录与 crate 形态（已达成 CLI-first）**
- 无 `lib.rs`：项目为纯二进制 crate，不提供库引用。
- 入口为 `src/main.rs`：声明各 mod，解析 CLI，无子命令时调用 `app::run(config)`。
- `src/app.rs`：仅含 `run(config)` 与路由注册（仅 JSON API 与网关路由），并 re-export `storage::hash_password`。
- `AppConfig` 与 `from_env()` 位于 `src/types.rs`；网关与管理逻辑分布在 `gateway`、`storage`、`provider`、`service`、`api_key`、`system`、`authz`、`dto`、`queries`、`mutations` 等单文件模块。
- Web UI 已移除，管理仅通过 CLI 与 `/api/admin/*` JSON 接口。

### 2. 运行入口与配置

#### 2.1 二进制入口 `src/main.rs`

使用 `clap` 定义 CLI，为“场景化管理 + 网关启动”提供统一入口：

- `unigateway serve [--bind] [--db]`：启动 HTTP 网关服务（仅 JSON API，无 Web UI）
- `unigateway quickstart`：按推荐场景一次性创建默认 service / provider / api-key（便于脚本或 AI 一键初始化）
- `unigateway service ...`：管理服务（`list` / `create` / `delete` 等）
- `unigateway provider ...`：管理 Provider（`list` / `add` / `delete` / `bind` 等）
- `unigateway api-key ...`：管理 API Key（`list` / `create` / `revoke` 等）
- `unigateway metrics`：从 DB 打印或导出 metrics 快照

CLI 子命令应支持机器可读输出（例如 `--format json`），以便 AI skills 或自动化脚本直接消费。

#### 2.2 配置结构 `AppConfig`

文件：`src/types.rs`

- 字段：`bind`、`db_url`、`enable_ui`（保留兼容，现无 UI）、`admin_token`、`openai_*` / `anthropic_*`（默认上游 Base URL、API Key、Model）。
- `from_env()`：从环境变量读取并给出合理默认值。

#### 2.3 运行主流程 `run(config)`

文件：`src/app.rs`

核心步骤：
- 若使用 sqlite：
  - 如果物理 DB 文件不存在，自动 `create`，避免首次启动失败。
- 初始化数据库连接：
  - `SqlitePoolOptions::new().max_connections(5).connect(&config.db_url)`
- 初始化 schema 与种子数据：
  - 调用 `storage::init_db(&pool).await?`
  - 若不存在 `admin` 用户，自动创建 `admin / admin123`
- 构建 `AppState`：
  - `pool`：SQLx 连接池
  - `config`：运行配置
  - `api_key_runtime`：内存中保存每个 API Key 的当前 QPS / 并发窗口状态
  - `service_rr`：服务级 Provider 轮询索引
- 构建 Axum 路由（CLI-first，默认只提供 JSON API 与网关入口）：
  - 公共健康与管理 API：
    - `/health`、`/metrics`、`/v1/models`
    - `/api/admin/services`、`/api/admin/providers`、`/api/admin/bindings`、`/api/admin/api-keys`
  - 网关入口：
    - `/v1/chat/completions` → `gateway::openai_chat`
    - `/v1/messages` → `gateway::anthropic_messages`
- 统一附加：
  - `.with_state(Arc::new(state))`
  - `TraceLayer::new_for_http()` 方便观测调用
  - 通过 `TcpListener::bind` + `axum::serve` 启动 HTTP 服务

### 3. CLI 管理模块 `src/cli.rs`

职责：提供不依赖 Web UI 的脚本化操作入口，主要面向 DevOps / CI。

核心函数：
- `init_admin(db_url, username, password)`：
  - 若无 `users` 表则创建
  - 使用 `hash_password` 写入 / 更新对应用户名的密码
- `create_service(db_url, service_id, name)`：
  - 确保管理 schema 存在
  - `INSERT OR REPLACE INTO services(id, name, routing_strategy, created_at...)`
- `create_provider(...) -> provider_id`：
  - 新增一条 Provider 记录，`endpoint_id / base_url / model_mapping` 可空
  - 返回自增的 `provider_id`，方便后续绑定
- `bind_provider(db_url, service_id, provider_id)`：
  - 在 `service_providers` 表插入一条绑定关系
- `create_api_key(...)`：
  - 在 `api_keys` 表中插入 / 更新 key 记录
  - 同时维护 `api_key_limits` 表中的 QPS / 并发限制
- `print_metrics_snapshot(db_url)`：
  - 打印 `request_stats` 的计数，用于简单 metrics 输出。

### 4. 协议与上游调用模块 `src/protocol.rs`

职责：在“网关请求 JSON”与 `llm_connector::ChatRequest / ChatResponse` 之间做适配，并统一调用上游。

- `UpstreamProtocol`：`OpenAi` / `Anthropic`
- `openai_payload_to_chat_request(payload, default_model)`：
  - 从 OpenAI 风格的 payload 中解析 `model`、`messages`、`temperature`、`top_p`、`max_tokens`、`stream` 等
  - 使用 `openai_messages` 将 `messages` 转成 `Vec<Message>`
- `anthropic_payload_to_chat_request(payload, default_model)`：
  - 解析 `system` + `messages`，注入为 `Message` 列表
- `invoke_with_connector(protocol, base_url, api_key, req)`：
  - 根据协议构造 `LlmClient::openai` 或 `LlmClient::anthropic_with_config`
  - 调用 `client.chat(req).await`
- `chat_response_to_openai_json(resp)` / `chat_response_to_anthropic_json(resp)`：
  - 将 `ChatResponse` 转成兼容 OpenAI / Anthropic 的 JSON 响应结构

这层是“协议适配层”，屏蔽上游 SDK 的接口差异，对网关其他代码只暴露统一的 `ChatRequest` / `ChatResponse` 视图。

### 5. SDK 客户端模块 `src/sdk.rs`

职责：给下游服务提供一个简单的 HTTP SDK。

- `UniGatewayClient`：
  - 字段：`base_url`、`api_key`（可选）、`http: reqwest::Client`
  - `openai_chat(&self, payload)`：
    - POST `${base_url}/v1/chat/completions`，必要时附加 Bearer 认证
  - `anthropic_messages(&self, payload)`：
    - POST `${base_url}/v1/messages`，带上 `anthropic-version` 与 `x-api-key`

使用方式：业务方只需要构造 `serde_json::Value` 形式的 OpenAI/Anthropic 请求体，交给此 SDK 转发即可。

### 6. 应用层核心：存储与网关逻辑

#### 6.1 应用状态 `src/types.rs`

- `AppState`：
  - `pool: SqlitePool`：所有 handler 共享的数据库连接
  - `config: AppConfig`：运行配置
  - `api_key_runtime: HashMap<String, RuntimeRateState>`：内存中维护每个 API Key 的 QPS / 并发窗口
  - `service_rr: HashMap<String, usize>`：`service_id + protocol` 级别的轮询游标
- `GatewayApiKey`：对应 `api_keys` + `api_key_limits` 联表结果
- `ServiceProvider`：对应 `providers` 表 + 部分字段
- `RuntimeRateState`：QPS 窗口和当前并发统计
- `LoginForm`：登录表单映射
- `ModelList / ModelItem`：用于 `/v1/models` 返回结构

#### 6.2 存储与路由辅助 `src/storage.rs`

核心职责：
- 初始化所有必要表结构（含 Users / Sessions / 请求统计 / 服务 / Provider / API Key / 限流表 / 请求日志）
- 提供网关用的查询与更新函数：
  - `init_db(pool)`：启动时保证表存在，并注入默认 `admin/admin123`
  - `hash_password(raw)`：简单的 SHA256 + hex 编码
  - `record_stat(pool, provider, endpoint, status_code, latency_ms)`：写入 `request_stats`
  - `find_gateway_api_key(pool, raw_key)`：
    - 联表 `api_keys` 与 `api_key_limits`
    - 统一返回 `GatewayApiKey`，附带限流参数
  - `select_provider_for_service(state, service_id, protocol)`：
    - 从 `service_providers + providers` 选出当前可用 Provider
    - 使用 `service_rr` 做轮询分发，实现简单的多 Provider 负载均衡
  - `map_model_name(model_mapping, requested_model)`：
    - 支持 JSON 形式的 `{"gpt-4o-mini": "...", "default": "..."}` 或简单字符串直写映射

#### 6.3 网关 Handler `src/gateway.rs`

核心接口：
- `openai_chat(State<AppState>, headers, Json<payload>) -> Response`
- `anthropic_messages(State<AppState>, headers, Json<payload>) -> Response`

主要流程（两者模式相同，仅协议 / header 名不同）：
1. **解析下游调用凭据**
   - 从下游请求 header 中解析：
     - OpenAI 风格：`Authorization: Bearer <token>`
     - Anthropic 风格：`x-api-key: <token>`
   - 若 header 为空，则退回到全局 `OPENAI_API_KEY` / `ANTHROPIC_API_KEY`
2. **构建标准化上游请求**
   - 使用 `*_payload_to_chat_request` 将 JSON payload 转为 `ChatRequest`
3. **判断是否走“网关 Key”模式**
   - 如果 header/参数中的 token 非空：
     - 通过 `find_gateway_api_key` 查询是否为网关管理的 key
     - 若是：
       - 校验 `is_active`
       - 校验 `quota_limit / used_quota`
       - 通过 `acquire_runtime_limit` 做内存级 QPS / 并发控制
       - 通过 `select_provider_for_service` 选出绑定的 Provider
       - 解析 Provider 的实际上游 base_url 与 api_key
       - 做模型名映射 `map_model_name`
       - 预先对 `api_keys.used_quota` 做 `+1` 更新
4. **确定最终上游调用参数**
   - `upstream_base_url`：优先 Provider 上配置的 `base_url` / `endpoint_id` → `llm_providers::get_endpoint`，否则用全局 env base_url
   - `upstream_api_key`：优先 Provider.api_key，其次全局 env
5. **调用上游 LLM**
   - 通过 `invoke_with_connector(protocol, &base_url, &api_key, &request).await`
6. **记录统计并生成响应**
   - 成功 / 失败都会调用 `record_stat`：
     - 记录 Provider 名、endpoint（`/v1/chat/completions` 或 `/v1/messages`）、状态码、耗时
   - 成功时将 `ChatResponse` 转换为兼容 OpenAI / Anthropic JSON
   - 出错时返回 4xx/5xx 与 JSON 错误体
   - 若使用网关 Key，结束时调用 `release_runtime_inflight` 递减并发计数

**限流实现 `acquire_runtime_limit` / `release_runtime_inflight`**
- 基于每个 API Key 的窗口状态：
  - `window_started_at`：当前 1 秒窗口起点
  - `request_count`：窗口内请求数
  - `in_flight`：当前并发请求数
- 逻辑：
  - 每次请求前如果窗口超过 1 秒则重置计数
  - 若 `qps_limit` > 0 且 `request_count >= qps_limit` → 429
  - 若 `concurrency_limit` > 0 且 `in_flight >= concurrency_limit` → 429
  - 成功获取后 `request_count += 1`，`in_flight += 1`
  - 请求结束时通过 `release_runtime_inflight` 将对应 `key` 的 `in_flight -= 1`

### 7. 管理与配置：CLI + JSON API

在“纯 CLI + JSON API”的目标形态下：

- 管理入口：
  - 首选通过 CLI 子命令对 service / provider / api-key / metrics 做增删改查。
  - 同时保留 `/api/admin/...` 这一组 JSON API，方便未来有需要时由上层编排服务或 AI 应用直接调用。
- 共享模块：
  - `dto.rs`：集中管理所有管理域的请求/响应/Row 结构体。
  - `queries.rs`：封装服务、Provider、API Key、Dashboard 统计、日志等查询。
  - `mutations.rs`：封装创建/删除/绑定等写入逻辑。
- CLI 与 JSON API 复用这套查询/写入逻辑，只是对外呈现形态不同（命令行 vs HTTP）。

### 8. 管理鉴权（当前形态）

- Web UI 与登录会话已移除。管理 API（`/api/admin/*`）仅通过 **x-admin-token** 头鉴权：若配置了 `UNIGATEWAY_ADMIN_TOKEN`，则请求头需带相同值；未配置则放行。
- `users`、`sessions` 表仍存在（如 InitAdmin 子命令会写 `users`），但不再用于 HTTP 管理接口的认证。
- `authz::is_admin_authorized` 只做 token 校验，无 Cookie/登录页。

### 9. 数据模型与表结构梳理

当前 SQLite 主要表：
- `users`：后台用户
- `sessions`：登录会话
- `request_stats`：请求级统计（provider、endpoint、status_code、latency_ms）
- `services`：逻辑服务（路由组）
- `providers`：上游 Provider 配置（类型、endpoint_id、base_url、api_key、model_mapping、权重、是否启用）
- `service_providers`：Service 与 Provider 的多对多绑定
- `api_keys`：下游访问网关的 API Key（配额 / 状态 / 过期时间）
- `api_key_limits`：API Key 的 QPS / 并发限制
- `request_logs`：更详细的请求日志（tokens、status_code、latency、client_ip 等）

### 10. 作为后续开发的参考要点

基于当前结构，后续演进时建议遵守：
- 网关主流程相关：
  - 只在 `gateway.rs` + `storage.rs` + `protocol.rs` 中扩展新能力（例如多模型路由策略、更多上游协议）
  - 所有与配额 / 限流相关的逻辑集中在 `gateway.rs` + `storage.rs`，避免散落到 Admin handler
- 管理相关：
  - 所有增删改查能力优先通过 CLI 子命令和 `/api/admin/...` JSON 接口暴露。
  - 新增查询 / 写入逻辑集中放入 `queries.rs` / `mutations.rs`，上层 handler 只负责调度。
- CLI / SDK：
  - 所有非 UI 管理操作优先考虑在 `cli.rs` 中提供子命令
  - SDK `UniGatewayClient` 若扩展到更多协议（如 embeddings），应保持与网关路由一一对应

这份文档可以作为后续开发时的「架构地图」：新增功能时先确定属于哪一层（网关核心 / Admin / SDK / CLI / UI），再落位到对应模块，避免逻辑混杂与文件再次膨胀。

