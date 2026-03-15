# UniGateway 面向个人开发者 / AI 重度使用者的开发规划

这份文档用于指导接下来几轮迭代，目标是把 UniGateway 从“通用轻量 LLM 网关”收敛为“个人开发者 / AI 重度使用者的统一模型入口”。

## 0. 当前进度更新（2026-03-15）

目前主线目标已经进入“核心能力已成形，继续做工具接入和产品化打磨”的阶段。

### 已完成的核心能力

- mode 导向 CLI：`ug mode list/show/use`
- 路由解释：`ug route explain`
- 集成模板输出：`ug integrations`
- 诊断与烟雾测试：`ug doctor`、`ug test`
- quickstart 默认生成 `fast` / `strong` / `backup`
- provider / model 数据改为基于 registry 动态生成
- OpenAI / Anthropic 默认启用 streaming

### 已完成的主要重构

- `cli.rs` 已拆分为 `modes / render / quickstart / diagnostics`
- `main.rs` 的 quickstart/setup 逻辑已拆到 `setup/`
- `gateway.rs` 已拆到 `gateway/chat.rs`、`gateway/streaming.rs`
- `protocol.rs` 已拆到 `protocol/client.rs`、`protocol/messages.rs`
- `config.rs` 已拆到 `config/schema.rs`、`store.rs`、`select.rs`、`admin.rs`
- `cli/render.rs` 已拆到 `cli/render/integrations.rs`、`cli/render/routes.rs`

### 当前工具支持状态

已经具备模板或显式支持的工具：

- Cursor
- Claude Code
- Codex
- OpenClaw
- Zed
- Droid
- OpenCode
- 通用 `env / python / node / curl / anthropic`

### 当前接入优先级

后续产品化打磨优先围绕下面这组工具推进：

1. OpenClaw
2. Zed
3. Claude Code
4. Cursor
5. Droid
6. OpenCode

### 当前判断

项目最核心的“统一本地入口 + mode 抽象 + 多上游路由 + 工具接入”方向已经实现，接下来的重点不再是大规模底层重写，而是：

- 把高优先级工具做成更低摩擦的一等公民集成
- 强化多 mode 工作流
- 继续提升诊断、解释和默认体验

## 1. 开发总目标

在接下来的开发中，我们只围绕一个核心目标推进：

**让同时使用多个 AI 工具、多个模型、多个 provider 的开发者，能够用最小配置成本获得统一接入、稳定切换和明确诊断能力。**

这意味着我们的优先级排序应当是：

1. 先把首个高频场景打透
2. 再补支撑这个场景的能力
3. 暂时停止会分散注意力的扩展方向

## 2. 近期开发原则

### 2.1 功能增加必须服务主场景

以后所有新增功能都先问一个问题：

**它是否直接帮助个人开发者更快接入、更稳定使用、更容易切换或更容易诊断？**

如果不是，就不应进入近期开发计划。

### 2.2 优先做“用户动作”，不是“底层资源 CRUD”

近期设计和实现应尽量围绕这些动作：

- 接入一个工具
- 配置一个常用模式
- 测试一个模式是否可用
- 解释一个模式会走到哪里
- 在 provider 故障时快速恢复

### 2.3 优先做可默认使用的方案

相较于复杂可配性，个人开发者更需要：

- 默认模式
- 默认 fallback
- 默认 provider 模板
- 默认验证方式

## 3. 产品阶段拆解

建议把开发计划拆成四个阶段。

## 阶段 0：方向收敛与模型重命名

### 目标

先把产品表达、概念层和命令组织收敛，避免后面边做边改。

### 交付物

- 明确对外产品语言：`mode`、`upstream`、`integration`、`doctor`
- 保留内部 `service/provider/binding`，但不再把它们作为主叙事
- 形成统一的命令命名方向
- 明确短期非目标列表

### 实施重点

- 梳理 CLI 顶层命令结构
- 定义 mode 与现有 service 的映射
- 定义 upstream profile 与现有 provider 的映射
- 确认 quickstart 的新用户路径

### 验收标准

- 团队内部能用统一词汇讨论需求与实现
- 后续文档、命令、实现不再混用多套概念

## 阶段 1：首个用户体验闭环

### 目标

让用户从安装到第一个工具接入成功，流程足够短、足够稳。

### 核心问题

当前项目已经有网关和路由能力，但“首个成功体验”还不够面向 AI 重度使用者，需要重构 quickstart 和工具接入路径。

### 交付物

- 重做 `ug quickstart`
- 自动生成默认 mode：`fast`、`strong`、`backup`
- 提供主流工具接入片段输出
- 提供一条立即可验证的测试命令

### 建议命令方向

- `ug quickstart`
- `ug integrations`
- `ug test`

### 代码工作项

- `main.rs`：调整命令组织和帮助文案
- `cli.rs`：重写 quickstart 逻辑，围绕 mode 生成配置
- `config.rs`：增加 mode 语义层
- `provider-examples` 相关逻辑：沉淀为可复用 provider preset

### 验收标准

- 新用户在几分钟内可以完成安装、配置、启动、验证
- 至少一个 AI 工具和一个脚本调用能顺利接入
- 用户不需要先理解 `service/provider/binding`

## 阶段 2：模式系统与可解释路由

### 目标

把“模式”真正做成产品核心抽象，而不只是配置别名。

### 交付物

- mode 列表与详情
- mode 对应的主上游和 fallback 上游
- 路由解释能力
- 更清晰的默认模型映射和 provider 选择逻辑

### 建议命令方向

- `ug mode list`
- `ug mode show <name>`
- `ug route explain <mode>`
- `ug mode use <name>`

### 代码工作项

- `config.rs`：扩展 mode 配置结构
- `routing.rs`：支持更清晰的 preference/fallback 解释
- `gateway.rs`：把 mode 解析融入请求生命周期
- `types.rs`：补充面向 mode 的共享类型

### 验收标准

- 用户可以清楚知道某个 mode 会走到哪些上游
- 模式切换不需要直接编辑复杂底层配置
- fallback 行为在文档和运行时都一致可解释

## 阶段 3：诊断、可靠性与日常运维体验

### 目标

让用户在日常使用中，不只是“能用”，而且“出了问题能快速知道为什么”。

### 交付物

- `ug doctor`
- `ug recent`
- provider 连通性检查
- mode 级别健康状态
- 最近错误摘要

### 建议命令方向

- `ug doctor`
- `ug recent`
- `ug doctor --provider <name>`
- `ug test <mode>`

### 代码工作项

- `system.rs`：扩展状态信息输出
- `gateway.rs`：补充轻量请求记录和失败摘要
- `config.rs` 或新模块：维护最近状态快照
- `cli.rs`：提供用户友好的诊断输出

### 验收标准

- 用户能区分是本地配置问题、网关问题还是上游问题
- 常见故障无需翻日志即可初步定位
- 路由和诊断输出对非基础设施工程师也友好

## 阶段 4：稳定性打磨与产品完成度提升

### 目标

在核心体验闭环建立后，补齐长期使用所需要的稳定性和边界能力。

### 交付物

- 更安全的默认配置
- 更可靠的错误提示和恢复建议
- 更稳定的配置演进方案
- 更成体系的测试覆盖

### 优先方向

- admin 默认安全收紧
- provider key / gateway key 的本地安全边界说明
- 配置迁移与兼容
- fallback、模型映射、诊断相关测试补强

### 验收标准

- 新老配置迁移路径清晰
- 常见核心流程具备自动化测试
- 默认行为更符合本地开发者场景

## 4. 明确要暂停或延后的方向

为了保证聚焦，下面这些方向近期不应抢占主线资源：

- 重 Web UI
- 企业级 RBAC
- 多租户账单系统
- 复杂调度算法
- 泛化 SDK 平台路线

这些方向不是永远不做，而是现在不做。

## 5. 近期实现顺序建议

如果按最小可用产品节奏推进，建议按下面顺序落地：

1. 定义 mode 语义与 CLI 命名
2. 重做 quickstart
3. 输出工具接入模板
4. 加入 `ug test` 与最小验证路径
5. 实现 `mode list/show` 与 `route explain`
6. 实现 `ug doctor`
7. 补齐状态快照与最近错误摘要
8. 最后再处理更深的稳定性和配置演进

这个顺序的核心原因是：

- 先打通用户入口
- 再让用户理解系统行为
- 再让用户解决故障
- 最后再扩展能力边界

## 6. 推荐的技术拆分方式

为了避免一次性大改，建议按照“语义层优先、存储层后移”的方式推进。

### 第一层：语义层改造

先在 CLI 和配置解释层引入：

- mode
- upstream profile
- integration template
- doctor snapshot

底层仍复用当前 `service/provider/binding`。

### 第二层：路由层改造

在 `routing.rs` 中把当前偏通用的路由能力，整理成更符合个人用户心智的：

- preference order
- fallback chain
- route explanation

### 第三层：诊断层补齐

在不引入重型依赖的前提下，加入：

- 最近请求摘要
- 最近失败原因
- provider 连通性探测
- mode 健康快照

### 第四层：存储层再演进

等语义层稳定后，再决定是否将配置结构正式迁移到 `modes/upstreams` 语义模型。

## 7. 代码模块建议分工

### `main.rs`

- 调整命令树
- 强化帮助信息与默认入口叙事

### `cli.rs`

- 承载 quickstart
- 承载 mode 管理命令
- 承载 tool integration 输出
- 承载 doctor / test / route explain

### `config.rs`

- 扩展 mode 语义
- 承载 upstream profile 解释逻辑
- 维护轻量状态快照

### `routing.rs`

- mode -> upstream chain 解析
- fallback 解释输出
- 默认路由决策简化

### `gateway.rs`

- 请求时按 mode 解析路由
- 写入轻量请求结果信息
- 为诊断命令提供运行时数据来源

### `system.rs`

- 保留健康与 metrics
- 增加更面向用户的状态输出能力

## 8. 测试策略建议

在这一方向下，测试重点也应调整。

### 优先补的测试

1. quickstart 生成的配置是否正确
2. mode 到路由链的映射是否正确
3. fallback 顺序是否符合预期
4. 路由解释输出是否正确
5. 诊断命令是否能覆盖常见故障

### 不够优先的测试

- 偏边缘的管理 CRUD 场景
- 当前主线之外的扩展 provider 细节

## 9. 成功标准

当以下条件成立时，说明这个方向开始站住：

- 新用户能快速接入至少一个 AI 工具
- 用户能用 mode 而不是底层资源概念完成日常切换
- 用户能在 provider 出问题时快速定位和恢复
- 文档、CLI、运行时输出都围绕统一心智模型展开

## 10. 结论

接下来的开发，不是继续把 UniGateway 往“更大更全的网关平台”推，而是把它做成一个真正对个人开发者有用的产品：

- 接入快
- 切换快
- 兜底稳
- 诊断清楚

这条路线一旦打透，后续无论是否扩展到团队场景，都会建立在更扎实的产品基础上。
