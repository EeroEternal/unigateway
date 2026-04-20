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
