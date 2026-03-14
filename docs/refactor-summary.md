# UniGateway 重构目标与当前状态总结

## 一、已达成目标

### 1. 极简、CLI-first

- **纯二进制**：已删除 `lib.rs` 与 Cargo `[lib]`，项目仅为单一可执行 crate，不提供库引用。
- **管理方式**：仅通过 **CLI 子命令** 与 **JSON API**（`/api/admin/*`）进行管理，不依赖 Web UI。
- **鉴权**：管理 API 使用 `x-admin-token` 头（或未配置时放行）；已移除基于 Cookie 的登录与会话。

### 2. 去掉 Web UI

- **删除**：整个 `src/ui/` 目录（`mod.rs`、`templates.rs`）及所有与 UI 相关的 `admin_*` 页面/局部刷新 handler。
- **保留**：仅保留 JSON 管理接口与网关路由：
  - `/health`、`/metrics`、`/v1/models`
  - `/api/admin/services`、`/api/admin/providers`、`/api/admin/bindings`、`/api/admin/api-keys`
  - `/v1/chat/completions`、`/v1/messages`
- **移除模块**：auth、dashboard、logs、settings、shell、render 等仅服务 UI 的模块已删除。

### 3. 目录扁平化

- **入口**：二进制入口为 `src/main.rs`，在 Cargo.toml 中配置为 `[[bin]] path = "src/main.rs"`。
- **无 app/bin/ui 目录**：已删除 `src/app/`、`src/bin/`、`src/ui/` 目录；功能模块全部位于 `src/` 根下。
- **当前 src 布局**：
  - `main.rs`：CLI 解析（clap）、子命令分发、无子命令时默认启动网关。
  - `app.rs`：薄层，仅含 `run(config)` 与路由注册（无 UI 路由），并 re-export `storage::hash_password`。
  - `types.rs`：`AppConfig`（含 `from_env()`）、`AppState` 及网关用类型。
  - `gateway.rs`、`storage.rs`、`dto.rs`、`queries.rs`、`mutations.rs`、`authz.rs`、`provider.rs`、`service.rs`、`api_key.rs`、`system.rs`：网关与管理逻辑。
  - `cli.rs`、`protocol.rs`、`sdk.rs`：CLI 实现、协议适配、可选 SDK。

### 4. CLI 子命令（当前）

- **Serve**：`--bind`、`--db`、`--no-ui`（现无 UI，保留兼容）。
- **InitAdmin**：初始化/更新 admin 用户密码。
- **Metrics**：从 DB 打印 metrics 快照。
- **CreateService** / **CreateProvider** / **BindProvider** / **CreateApiKey**：对应管理能力。

场景与路由设计仍参见 `usage-scenarios-and-routing-design.md`；后续可在此基础上增加 quickstart、嵌套子命令（如 `service list`）等。

## 二、文档与代码对应关系

- **架构与目录**：以本文与更新后的 `directory-structure.md`、`project-architecture.md` 为准。
- **CLI 目标形态**：`cli-design.md` 描述目标 CLI 结构（含 quickstart、`--format json` 等），当前实现为子集。
- **场景与路由**：`usage-scenarios-and-routing-design.md` 仍适用，仅管理入口改为 CLI + JSON API。
