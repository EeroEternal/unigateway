# UniGateway 文档索引

面向使用者与贡献者的文档按目录拆分：`design`（产品与协议设计）、`guide`（配置与接入）、`dev`（路线图与贡献者速查）。

## design

| 文件 | 说明 |
| --- | --- |
| [`arch.md`](design/arch.md) | 产品壳 / runtime / core 三层架构与请求路径 |
| [`cli.md`](design/cli.md) | CLI 优先的产品与命令结构草案 |
| [`admin.md`](design/admin.md) | 无内置 Web UI 时的 `/api/admin/*` 集成约定 |
| [`queue.md`](design/queue.md) | 网关层并发排队与背压（`middleware`） |
| [`scheduling.md`](design/scheduling.md) | 调度与队列能力的中长期演进设想 |

## guide

| 文件 | 说明 |
| --- | --- |
| [`config.md`](guide/config.md) | `unigateway.toml` 字段与同步到 core 的规则 |
| [`providers.md`](guide/providers.md) | 常见 Provider 的 TOML 与调用示例 |
| [`embed.md`](guide/embed.md) | 在其它 Rust 应用中嵌入 `unigateway-core` / `runtime` |

## dev

| 文件 | 说明 |
| --- | --- |
| [`memory.md`](dev/memory.md) | 贡献者与 AI 代理用的代码心智模型与入口文件 |
| [`roadmap.md`](dev/roadmap.md) | 产品阶段与近期优先级（会随迭代更新） |
| [`local-gateway.md`](dev/local-gateway.md) | 本地网关定位下的适配、观测、CLI 强化待办 |
| [`openclaw.md`](dev/openclaw.md) | OpenClaw 与本地网关联动的示例流程 |

更通用的代理协作约定见仓库根目录 [`AGENTS.md`](../AGENTS.md)。
