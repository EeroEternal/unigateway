# AGENTS.md

本文件为 AI 代理与自动化工具（Copilot、MCP、CLI 代理等）在本仓库内协作时的约定，风格参考常见 Rust 开源项目（例如 [Azure SDK for Rust](https://github.com/Azure/azure-sdk-for-rust/blob/main/AGENTS.md)）与社区总结的 [Rust 代理编码守则](https://gist.github.com/minimaxir/068ef4137a1b6c1dcefa785349c91728)。

## 仓库概览

UniGateway 是面向嵌入方的本地优先 LLM **库 workspace**：TOML 配置状态、OpenAI/Anthropic 协议与执行引擎以 crate 形式交付；HTTP、CLI、用户与租户管理由宿主应用（例如独立网关产品）自行实现。

- **语言**：Rust（workspace edition 见根 `Cargo.toml`）
- **MSRV**：各成员 crate 的 `Cargo.toml` 中的 `rust-version`
- **Workspace 成员**：`unigateway-core`（执行引擎与协议驱动）、`unigateway-host`（host 桥与执行封装）、`unigateway-protocol`（协议翻译与中立响应）、`unigateway-config`（配置持久化、mutation、core pool 投影）、`unigateway-sdk`（对外门面 re-export）

## 目录结构（简）

```text
unigateway-sdk/           # 对外门面（re-export core / protocol / host）
unigateway-config/        # GatewayState、schema、persist、mutation、core sync
unigateway-host/          # HostContext、执行封装、host contract
unigateway-protocol/      # 协议请求解析、响应格式化、中立 HTTP 响应
unigateway-core/          # UniGatewayEngine、路由、驱动、上游请求
docs/design/              # 架构、admin、排队与调度设计
docs/guide/               # 配置格式、Provider 示例、嵌入指南
docs/dev/                 # 路线图、贡献者 memory、集成草稿（`dev/` 下单文件宜短名 kebab-case，如 `embed-sdk.md`）
```

## 文档入口

- 人类与代理的快速心智模型：[`docs/dev/memory.md`](docs/dev/memory.md)
- 架构总览：[`docs/design/arch.md`](docs/design/arch.md)
- 全文索引：[`docs/README.md`](docs/README.md)
- 嵌入栈与 SDK 门面规划：[`docs/dev/embed-sdk.md`](docs/dev/embed-sdk.md)

## UniGateway 边界（强约束）

以下边界是本仓库的硬约束；代理与维护者都不应为了某个上层产品的短期需求而突破。

- **UniGateway MUST 保持为可嵌入的库 workspace**，提供执行引擎、协议翻译、host bridge、配置投影与中立扩展点；它不是一个内建完整产品壳的 opinionated gateway framework。
- **UniGateway MUST 优先下沉中立原语**：例如 request / attempt / stream 生命周期事件、标准 report 类型、错误分类入口、配置模型、可注入策略接口、与 runtime 无关的执行抽象。
- **UniGateway MUST NOT 吸收宿主产品职责**：包括但不限于 HTTP server、CLI 产品壳、用户与租户管理、认证鉴权、预算与计费、后台管理 API、数据库持久化、审计落库、异步任务编排、运营面板与产品化工作流。
- **UniGateway MUST NOT 内置业务语义或产品策略**：例如强产品倾向的评分公式、路由权重规则、租户规则、价格规则、配额规则、风控规则、后台聚合逻辑。仓库内只允许中立接口与可替换策略槽，不允许把具体业务决策写死到 core / host / protocol crate。
- **UniGateway crate 之间 MUST 避免对上层应用语义产生反向耦合**：不要把某个宿主应用、某类产品后台、某个特定业务策略模块、某个特定存储模型或某个特定 Web 框架假设引入到公共 crate 的 API 或依赖中。
- **当新增能力时，若该能力同时需要“原始事件”和“业务解释”两层含义，UniGateway 只负责前者**；后者必须留在宿主层或额外的集成层实现。
- **当边界不清楚时，默认做更窄的 UniGateway**：宁可只暴露 hook、report、trait、metadata 和反馈入口，也不要把上层产品逻辑提前沉入本仓库。

一个简单判断标准：

- 如果某能力脱离具体宿主产品后仍然是通用、可复用、运行时无关的库能力，可以考虑进入 UniGateway。
- 如果某能力依赖产品策略、租户语义、运营规则、存储模型或后台展示，则它不属于 UniGateway，应留在宿主或单独集成层。

## 构建与测试

```bash
cargo build
cargo test --workspace
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
```

修改任意 `.rs` 后应在提交前跑通 `fmt` 与 `clippy`（与 Azure 等仓库惯例一致）。

GitHub Actions 还会在 `ubuntu-latest` 上执行两组命令，发布前不要只依赖本机结果：

```bash
# .github/workflows/rust.yml 的 build job
cargo fmt -- --check
cargo clippy -- -D warnings
cargo build --verbose
cargo test --verbose

# .github/workflows/release.yml 的 verify job
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

如果改动包含 host / protocol / root crate，发布前至少手动对齐一次上述 GitHub Linux 命令；不要默认 macOS 本地通过就等于 Ubuntu CI 通过。

## 编码约定（摘要）

- 遵循 `rustfmt` 默认风格；公共 API 使用 `///` 文档注释，说明参数、返回值与可恢复错误。
- 库代码（尤其 `unigateway-core`）避免无说明的 `.unwrap()`；应用层可用 `anyhow` 等组合错误上下文。
- 优先用类型系统表达不变量；错误用 `Result` 与 `?` 向上传播。
- 避免通配符 `use`（测试模块 `use super::*` 除外）；导入顺序：标准库、外部 crate、`crate::` / `super::`。
- 并发与异步：异步路径不阻塞 runtime；CPU 密集逻辑考虑 `spawn_blocking` 或独立任务。
- 安全：不在仓库中提交密钥、token 或真实用户数据；敏感配置走环境变量或本地配置文件（且勿提交后者）。

## 推荐代理行为

- 改动前阅读 `docs/dev/memory.md` 中与任务相关的「请求生命周期」「config → pool 投影」小节。
- 小步提交、保持 diff 聚焦需求；不顺带大段无关格式化或「顺手重构」。
- 为新行为补充或更新 `#[cfg(test)]` 测试；对外部 HTTP 使用 mock 或可控替身。
- 文档与注释随代码同步更新；用户可见行为变化时考虑更新 `README.md` 或 `docs/guide/`。
- 发布新版本时，先把 release commit 推到 `main` 并确认 `Rust` workflow 在 GitHub Linux 上变绿，再创建并推送 `v*` tag；不要先推 tag 再补 `main` 上的 CI 修复。
- 对带 guard 的 `match` 分支，如果错误值本身不会被使用，写成 `Err(_)` 或 `_error`；不要保留未使用绑定。GitHub Linux 上的 `cargo clippy -- -D warnings` 会把这类问题直接卡死。

## 不建议的行为

- 跳过 `cargo test` / `clippy` 即宣称完成。
- 引入未声明用途的重依赖或改变公共 API 而不在 PR/说明中解释。
- 在 issue/注释中粘贴真实 API key。

## 提交前自检（清单）

- [ ] `cargo test --workspace` 通过
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` 无告警
- [ ] `cargo fmt --all -- --check` 通过
- [ ] 如准备发版，已额外跑过 `.github/workflows/rust.yml` 中的 `cargo fmt -- --check`、`cargo clippy -- -D warnings`、`cargo build --verbose`、`cargo test --verbose`
- [ ] 如准备发版，`main` 上对应提交的 GitHub `Rust` workflow 已通过，再推 `v*` tag
- [ ] 未包含密钥、token、或本机私密路径
- [ ] 与改动相关的文档链接仍有效（`docs/` 下路径）

---

维护者可随版本更新 MSRV、CI 命令与文档链接；代理以根 `Cargo.toml` 与 `.github/workflows` 为准。
