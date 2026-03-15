# UniGateway 面向个人开发者 / AI 重度使用者的架构设计

这份文档描述 UniGateway 在聚焦“个人开发者 / AI 重度使用者”之后的目标架构。这里不再把 UniGateway 视为通用网关平台，而是把它定义为一个**本地优先、工具友好、统一接入、多上游可切换的模型入口层**。

## 1. 产品定位

新的核心定位是：

**为同时使用多个 AI 工具、多个模型、多个上游 provider 的开发者，提供一个统一、稳定、低摩擦的本地模型入口。**

这意味着我们优先服务的不是“平台管理员”，而是：

- 在 Cursor、Codex、Claude Code、ChatGPT Agent 之间来回切换的人
- 需要同时使用 OpenAI、Anthropic、DeepSeek、Groq 等多个 provider 的人
- 希望写脚本、配工具、切模型时都复用同一个入口的人

## 2. 核心设计目标

围绕这类用户，架构必须优先满足下面五件事：

1. **统一入口**：多个工具和脚本都通过同一个 base URL 接入
2. **模式切换**：用户用“快 / 强 / 便宜 / 备用”这样的模式思考，而不是直接管理底层 provider
3. **稳定兜底**：某个上游失败时，网关自动 fallback，尽量不打断工作流
4. **极低接入成本**：安装、配置、验证必须足够快
5. **可理解、可诊断**：用户能知道当前模式对应什么、请求最终打到了哪里、失败原因是什么

## 3. 用户心智模型

为了降低复杂度，UniGateway 的对外心智模型应该从“资源管理”切换到“使用模式管理”。

### 用户不应该优先看到的概念

- service
- provider
- binding
- routing_strategy

这些概念在内部仍然有效，但不适合作为个人开发者的第一层体验。

### 用户应该优先看到的概念

- **接入入口（endpoint）**：给工具和脚本配置的地址
- **模式（mode）**：`default`
- **上游（upstream）**：OpenAI、Anthropic、DeepSeek、Groq 等
- **工具接入（integration）**：Cursor、Codex、Claude Code、脚本
- **诊断（diagnostics）**：连通性、当前路由、最近失败信息

也就是说，内部依然可以是 `service → provider → binding`，但外部主交互应当是：

`tool -> mode -> route -> upstream`

## 4. 目标产品结构

面向这一人群，UniGateway 的产品结构应当收敛成四个面：

## 4.1 工具接入面

这是用户最先看到的能力。

目标是让用户在安装后尽快完成这些事：

- 复制一个 Cursor 配置片段
- 复制一个 Codex 配置片段
- 复制一个 Claude Code 配置片段
- 用 curl / Python / Node 验证统一入口可用

这个层面对用户暴露的重点不是 provider 管理，而是：

- `ug quickstart`
- `ug integrations`
- `ug test`

## 4.2 模式管理面

这一层是产品的核心抽象。

用户不是直接选择 provider，而是选择“当前要什么”：

- `default`：统一工作流入口

模式是用户和底层路由系统之间的桥梁。

### 模式与内部模型的映射

建议保留当前的内部资源模型，但做一层语义映射：

- `mode` 映射为一个对外可见的逻辑服务
- `upstream profile` 映射为内部 provider
- `mode routes` 映射为绑定和路由策略

可理解成：

- `service` 是实现层概念
- `mode` 是产品层概念

CLI 和文档优先使用 `mode`，内部仍可复用 `service` 实现。

## 4.3 上游策略面

这一层处理真实 provider 的组织与兜底，但应保持尽量简单。

### 推荐的策略集合

个人开发者场景下，不需要先做复杂调度，只需要三种足够可解释的策略：

1. **single**：固定使用一个上游
2. **fallback**：主上游失败时依次尝试备用上游
3. **ordered preference**：按用户偏好顺序选择上游

当前项目已有 `round_robin` 和 `fallback` 能力。聚焦该场景后，建议把默认思路从“负载均衡”转为“偏好顺序 + fallback”。

原因很简单：

- 个人用户更关心结果稳定，不太关心多活流量分摊
- “为什么这次走了这个 provider”必须容易解释
- 优先级比权重更符合个人用户的理解方式

## 4.4 诊断与可观测面

这类用户不需要企业监控平台，但非常需要轻量的自解释能力。

最有价值的不是大盘，而是下面这些直接问题：

- 我的配置是不是有效？
- 某个 provider 现在能不能连通？
- `fast` 模式当前会打到哪里？
- 刚才失败是网关问题还是上游问题？
- 最近几次请求分别命中了哪个上游？

因此架构里必须明确保留一个独立的诊断层，而不是只提供 `/metrics`。

## 5. 目标领域模型

建议对外形成下面这套领域模型：

### 5.1 Gateway

本地运行的统一入口，负责：

- 提供 OpenAI / Anthropic 兼容接口
- 承载模式解析和路由
- 聚合请求日志和轻量诊断信息

### 5.2 Mode

用户可见的核心抽象，代表一类访问意图。

示例：

- `default`
- `script-default`

一个 mode 需要具备：

- 名称
- 描述
- 默认模型别名
- 上游优先级列表
- fallback 规则

### 5.3 Upstream Profile

代表一个上游能力配置，而不是单纯的 provider 记录。

建议包含：

- provider 名称
- provider 类型
- endpoint/base_url
- api key
- 推荐模型
- 支持能力标签（chat、embedding、reasoning、fast、cheap 等）

这样做的目的是让后续模式构建更自然，不必直接从生硬的 provider 字段出发。

### 5.4 Tool Integration

描述一个 AI 工具如何接入 UniGateway。

例如：

- Cursor integration
- Codex integration
- Claude Code integration
- Generic OpenAI-compatible integration

每个 integration 都应该能生成：

- 接入说明
- 可复制配置片段
- 验证命令

### 5.5 Diagnostic Snapshot

一份面向用户的轻量状态快照，包含：

- 当前 mode 列表
- 每个 mode 的主上游与备用上游
- 最近成功/失败请求
- 最近错误原因摘要
- 连通性检查结果

## 6. 目标配置结构

配置文件仍可基于 TOML，但建议逐步演进为更贴近用户语义的结构。

### 对外目标结构

```toml
[gateway]
bind = "127.0.0.1:3210"

[[upstreams]]
name = "openai-main"
provider = "openai"
base_url = "https://api.openai.com"
api_key = "sk-..."
capabilities = ["chat", "strong"]

[[upstreams]]
name = "deepseek-fast"
provider = "openai"
base_url = "https://api.deepseek.com"
api_key = "sk-..."
capabilities = ["chat", "fast", "cheap"]

[[modes]]
name = "default"
primary = ["deepseek-fast"]
fallback = ["openai-main"]
default_model = "fast-default"
```

### 与当前实现的兼容策略

当前项目已经有：

- `services`
- `providers`
- `bindings`
- `api_keys`

短期不需要重写底层存储，而是建议：

- `mode -> service`
- `upstream profile -> provider`
- `mode route -> bindings + routing_strategy + priority`

先做语义别名层，再考虑物理结构迁移。

## 7. 运行时架构

目标运行时由五个层次组成。

### 7.1 CLI Experience Layer

职责：

- quickstart
- 模式管理
- 工具接入模板输出
- 诊断命令

建议新增或强化的命令方向：

- `ug quickstart`
- `ug mode list`
- `ug mode use <name>`
- `ug integrations`
- `ug doctor`
- `ug route explain <mode>`
- `ug test <mode>`

### 7.2 Config / Profile Layer

职责：

- 加载模式和上游配置
- 处理 provider 预设
- 维护用户可理解的别名

这一层可以基于 `config.rs` 扩展，而不是另起炉灶。

### 7.3 Routing Layer

职责：

- 根据 mode 解析实际路由链
- 结合模型映射选择上游模型
- 处理 fallback 顺序

当前 `routing.rs` 已有基础。新的重点不是新增复杂算法，而是让结果更可预测、可解释。

### 7.4 Gateway Handler Layer

职责：

- 维持 OpenAI / Anthropic 兼容接口
- 执行模式到上游的最终调用
- 在响应中保留可诊断信息钩子

`gateway.rs` 和 `protocol.rs` 仍然是这层的核心。

### 7.5 Diagnostics Layer

职责：

- 健康检查
- 最近请求记录
- 上游连通性探测
- 路由解释

这一层是聚焦个人开发者后必须补齐的架构部分。

## 8. 关键流程设计

## 8.1 首次接入流程

理想流程：

1. 用户安装 `ug`
2. 执行 `ug quickstart`
3. 选择 1~2 个常用上游
4. 自动生成 `default` 模式
5. 输出工具接入片段
6. 运行一条验证命令确认成功

这是产品最重要的关键路径。

## 8.2 日常切换流程

理想流程：

1. 用户决定当前工作偏向速度、质量或成本
2. 在工具配置或请求中选择 mode
3. UniGateway 自动选择最合适的上游链
4. 用户可随时用 `ug route explain` 查看当前实际路由

## 8.3 故障处理流程

理想流程：

1. 用户感知请求失败或变慢
2. 运行 `ug doctor` 或 `ug recent`
3. 看到失败发生在网关、本地配置、还是上游 provider
4. 根据建议切换模式或禁用某上游

## 9. 非目标

为了保证聚焦，这个方向下有一些明确非目标：

- 不优先做重 Web UI
- 不优先做复杂企业权限系统
- 不优先做多租户计费平台
- 不优先做复杂流量调度算法

这些都可能未来有价值，但不应影响当前主线。

## 10. 与当前代码结构的关系

当前代码结构并不需要推翻，主要是产品语义和 CLI 体验要重组：

- `main.rs`：承担新的命令入口组织
- `cli.rs`：成为模式管理、工具接入、诊断的主要承载点
- `config.rs`：扩展模式和上游语义层
- `routing.rs`：从“策略选择”转向“偏好顺序 + fallback 可解释”
- `gateway.rs`：在响应路径里保留更多路由可观测性
- `system.rs`：从纯 metrics 扩展到更贴近个人使用者的状态信息

换句话说，这次聚焦更像是一次**产品抽象层重构**，不是底层重写。

## 11. 结论

聚焦个人开发者 / AI 重度使用者之后，UniGateway 的目标架构应该是：

- 对外是“统一入口 + 模式管理 + 工具接入 + 诊断”
- 对内是“现有网关能力 + 更强的语义层 + 更好的 CLI 体验”

这样既能保留当前项目已有的工程积累，也能让产品表达真正对准目标用户，而不是继续停留在一个偏底层的通用能力集合上。
