## UniGateway CLI-first 设计草案

### 1. 设计目标

- 把 UniGateway 明确定位为：**“HTTP 网关 + 场景化 CLI 管理工具”**，不再默认依赖 Web 管理界面。
- 所有管理动作（创建/查看/删除 service、provider、api-key，以及查看 metrics）都可以通过命令行完成，并且：
  - 既适合人类手动操作（参数友好、帮助信息详尽）；
  - 也适合 AI 应用 / 脚本自动调用（稳定的子命令与可解析输出）。

### 2. 顶层命令结构（基于 clap）

建议的 CLI 结构：

- `unigateway serve [FLAGS] [OPTIONS]`
  - 作用：启动 HTTP 网关服务。
  - 关键参数：
    - `--bind <ADDR>`：监听地址，覆盖 `UNIGATEWAY_BIND`。
    - `--db <URL>`：数据库 URL，覆盖 `UNIGATEWAY_DB`。
- `unigateway quickstart [OPTIONS]`
  - 作用：按推荐场景一键初始化：
    - 创建默认 `service`（如 `default`）；
    - 创建默认 `provider`（根据给定的 provider_type / endpoint_id / base_url / api_key）；
    - 绑定 service 与 provider；
    - 创建一个 API Key 并输出。
- `unigateway service <SUBCOMMAND>`
  - `list`：列出所有 service。
  - `create --id <ID> --name <NAME>`：创建/更新 service。
  - `delete --id <ID> [--force]`：删除 service（可选级联删除相关 key）。
- `unigateway provider <SUBCOMMAND>`
  - `list`：列出所有 provider。
  - `add --name <NAME> --type <TYPE> --endpoint-id <ID> [--base-url <URL>] --api-key <KEY> [--model-mapping <JSON>]`：新增 provider。
  - `delete --id <ID>`：删除 provider。
  - `bind --service-id <SID> --provider-id <PID>`：绑定 service 与 provider。
- `unigateway api-key <SUBCOMMAND>`
  - `list`：列出所有 API Key。
  - `create --name <NAME> [--service-id <SID>] [--provider-id <PID>...] [--quota-limit <N>] [--qps-limit <F>] [--concurrency-limit <N>]`：
    - 如果未指定 `service-id`，则自动创建一个 service；
    - 如果指定多个 `provider-id`，则自动为该 service 绑定这些 provider。
  - `revoke --key <KEY> [--delete-service]`：删除 API Key；可选删除其绑定的 service。
- `unigateway metrics [--db <URL>]`
  - 作用：从数据库中打印/导出基础 metrics（总请求数、分 endpoint 请求数等）。

可以在 `cli.rs` 中使用 `clap::Parser` + `Subcommand` 实现上述层级结构，并通过 match 分派到内部逻辑函数。

### 3. 输出格式与 AI/脚本友好性

为方便 AI/脚本消费，建议：

- 所有“查询类”命令（`list` / `metrics` 等）默认输出人类友好的表格或文本，同时支持：
  - `--format json`：以 JSON 数组或对象形式输出；
  - `--format plain`：适合 shell 管道处理的简洁文本。
- 所有“创建类”命令（例如 `api-key create`、`provider add`）在 JSON 输出下，应返回结构化字段：

```json
{
  "success": true,
  "data": {
    "service_id": "svc-xxx",
    "service_name": "My Service",
    "provider_id": 1,
    "api_key": "sk-...."
  }
}
```

这样 AI 只需调用命令并解析 stdout 即可完成自动化编排。

### 4. 与 HTTP 管理 API 的关系

- CLI 子命令优先直接调用 `sqlx` + `queries.rs` / `mutations.rs` 中的函数，不必通过 HTTP。
- `/api/admin/...` JSON 接口仍然保留，主要供：
  - 未来如需 Web UI 或远程管理时使用；
  - 外部服务/平台直接集成时使用。
- 逻辑复用关系：
  - HTTP handler 与 CLI 都只做“参数解析 + 调用共享查询/写入函数 + 格式化输出”。

### 5. 目录结构与模块分工（当前形态）

- 无 `src/app/` 目录：`src/app.rs` 仅含 `run()` 与路由注册；`AppConfig` 在 `types.rs`；网关与管理逻辑在 `src/` 根下各单文件模块。
- `src/cli.rs`：命令行解析与调度，依赖 `app::hash_password` 及直接使用 DB 的 create/bind/print_metrics 等。
- `src/ui/` 已删除：无 Web UI，管理仅 CLI + `/api/admin/*` JSON API。

### 6. 下一步落地顺序建议

1. 在 `cli.rs` 中补全/调整子命令结构，使其与本文描述一致（即便部分子命令先实现“最小可用子集”）。
2. 为关键子命令加上 `--format json` 支持，并约定统一的 JSON 响应结构。
3. 在 `README.md` 和 `project-architecture.md` 中，把 CLI-first 作为推荐用法，Web UI 标记为“计划移除的可选功能”。

