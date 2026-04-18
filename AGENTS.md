# AGENTS.md

本文件为 AI 代理与自动化工具（Copilot、MCP、CLI 代理等）在本仓库内协作时的约定，风格参考常见 Rust 开源项目（例如 [Azure SDK for Rust](https://github.com/Azure/azure-sdk-for-rust/blob/main/AGENTS.md)）与社区总结的 [Rust 代理编码守则](https://gist.github.com/minimaxir/068ef4137a1b6c1dcefa785349c91728)。

## 仓库概览

UniGateway 是面向个人开发者与重度用户的本地优先 LLM 网关：单进程、TOML 配置、OpenAI/Anthropic 兼容 HTTP 面，CLI 为产品主入口。

- **语言**：Rust（workspace edition 见根 `Cargo.toml`）
- **MSRV**：根 `Cargo.toml` 中的 `rust-version`
- **Workspace 成员**：`unigateway-core`（执行引擎与协议驱动）、`unigateway-runtime`（runtime 桥与响应翻译）、根 crate `unigateway`（HTTP、CLI、配置与 admin）

## 目录结构（简）

```text
src/                      # 产品壳：CLI、HTTP、配置、网关中间件、薄 handler
unigateway-runtime/       # RuntimeContext、执行封装、流式/协议适配
unigateway-core/          # UniGatewayEngine、路由、驱动、上游请求
docs/design/              # 架构、CLI、admin、排队与调度设计
docs/guide/               # 配置格式、Provider 示例、嵌入指南
docs/dev/                 # 路线图、贡献者 memory、集成草稿
```

## 文档入口

- 人类与代理的快速心智模型：[`docs/dev/memory.md`](docs/dev/memory.md)
- 架构总览：[`docs/design/arch.md`](docs/design/arch.md)
- 全文索引：[`docs/README.md`](docs/README.md)

## 构建与测试

```bash
cargo build
cargo test --workspace
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
```

修改任意 `.rs` 后应在提交前跑通 `fmt` 与 `clippy`（与 Azure 等仓库惯例一致）。

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

## 不建议的行为

- 跳过 `cargo test` / `clippy` 即宣称完成。
- 引入未声明用途的重依赖或改变公共 API 而不在 PR/说明中解释。
- 在 issue/注释中粘贴真实 API key。

## 提交前自检（清单）

- [ ] `cargo test --workspace` 通过
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` 无告警
- [ ] `cargo fmt --all -- --check` 通过
- [ ] 未包含密钥、token、或本机私密路径
- [ ] 与改动相关的文档链接仍有效（`docs/` 下路径）

---

维护者可随版本更新 MSRV、CI 命令与文档链接；代理以根 `Cargo.toml` 与 `.github/workflows` 为准。
