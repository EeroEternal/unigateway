# UniGateway 目录结构说明

## 当前目录结构（CLI-first、无 UI、扁平化）

```
unigateway/
├── .github/workflows/
├── Cargo.toml
├── Cargo.lock
├── README.md
├── rust-toolchain.toml
├── docs/
│   ├── directory-structure.md   # 本文件
│   ├── project-architecture.md
│   ├── usage-scenarios-and-routing-design.md
│   ├── cli-design.md
│   ├── refactor-summary.md
│   ├── admin-refactor-plan.md   # 历史规划，已由扁平化替代
│   └── app-modules-flat.md      # 历史规划，已落地为根下扁平结构
│
└── src/
    ├── main.rs       # 二进制入口：clap CLI、子命令分发，无子命令时默认 run()
    ├── app.rs        # run(config)、路由注册、pub use storage::hash_password
    ├── types.rs      # AppConfig、AppState、GatewayApiKey 等
    ├── gateway.rs    # openai_chat、anthropic_messages
    ├── storage.rs    # init_db、hash_password、网关用查询与限流数据
    ├── system.rs     # health、metrics、models
    ├── authz.rs      # is_admin_authorized（x-admin-token）
    ├── dto.rs        # 管理 API 请求/响应与 Row 结构
    ├── queries.rs    # 管理侧只读查询
    ├── mutations.rs  # 管理侧写入
    ├── provider.rs   # api_list_providers、api_create_provider、api_bind_provider
    ├── service.rs   # api_list_services、api_create_service
    ├── api_key.rs   # api_list_api_keys、api_create_api_key
    ├── cli.rs        # init_admin、create_service、create_provider、bind_provider、create_api_key、print_metrics_snapshot
    ├── protocol.rs   # OpenAI/Anthropic 协议转换与上游调用
    └── sdk.rs        # UniGatewayClient（可选）
```

## 设计要点

- **无 lib**：仅单一 binary，无 `src/lib.rs`。
- **无 app/bin/ui 目录**：所有模块为 `src/*.rs` 单文件，入口为 `main.rs`。
- **管理仅 CLI + JSON API**：无 Web UI；管理鉴权为 `x-admin-token`。
- **路由**：在 `app.rs` 的 `run()` 中注册 `/health`、`/metrics`、`/v1/models`、`/api/admin/*`、`/v1/chat/completions`、`/v1/messages`。

更细的架构说明见 `project-architecture.md`，重构目标与状态见 `refactor-summary.md`。
