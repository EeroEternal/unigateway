# UniGateway 文档索引

本仓库为 **Rust 库 workspace**（无内置 HTTP/CLI 二进制）。文档按目录拆分：`design`（协议与架构）、`guide`（配置与嵌入）、`dev`（贡献者与路线图）。

## design

| 文件 | 说明 |
| --- | --- |
| [`arch.md`](design/arch.md) | 当前库分层、config → core 投影、host 请求链（无 `src/` 产品壳） |
| [`cli.md`](design/cli.md) | **已弃用**：原 CLI 产品草案；管理面由宿主应用实现 |
| [`admin.md`](design/admin.md) | `/api/admin/*` JSON 约定（供自建管理网关或 UI 参考） |
| [`queue.md`](design/queue.md) | 并发排队与背压（实现位于 `unigateway-config::runtime`；HTTP 层由宿主接入） |
| [`scheduling.md`](design/scheduling.md) | 调度与队列能力的中长期设想 |

## guide

| 文件 | 说明 |
| --- | --- |
| [`config.md`](guide/config.md) | `unigateway.toml` 字段与同步到 core 的规则 |
| [`providers.md`](guide/providers.md) | 常见 Provider 的 TOML 与调用示例 |
| [`embed.md`](guide/embed.md) | 在其它 Rust 应用中嵌入（配合 `dev/embed-sdk.md`） |

## dev

| 文件 | 说明 |
| --- | --- |
| [`memory.md`](dev/memory.md) | 贡献者与 AI 代理用的心智模型与代码入口 |
| [`embed-sdk.md`](dev/embed-sdk.md) | `unigateway-sdk` 门面与对外 API 演进 |
| [`roadmap.md`](dev/roadmap.md) | 产品/库阶段与优先级（随迭代更新） |
| [`refactor-baseline.md`](dev/refactor-baseline.md) | **历史**：拆分过程与结构债记录；根 `src/` 已删除，阅读时以 `arch.md` 为准 |
| [`local-gateway.md`](dev/local-gateway.md) | **历史/设想**：本地网关 CLI 与观测增强（需由宿主应用实现） |
| [`openclaw.md`](dev/openclaw.md) | 与 OpenClaw 联动的示例流程（假设存在兼容的 HTTP 网关） |

更通用的代理协作约定见仓库根目录 [`AGENTS.md`](../AGENTS.md)。
