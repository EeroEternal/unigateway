# Admin 模块优化路径

> **说明**：本文为历史规划文档。当前已采用「去掉 UI + 扁平化 src」方案：无 `app/` 目录，管理仅保留 JSON API 与 CLI，详见 `refactor-summary.md` 与 `directory-structure.md`。

## 目标

当前 `src/app/admin.rs` 同时承担了过多职责：

- 页面路由处理
- 管理 API 处理
- 鉴权判断
- SQL 查询与写入
- HTML 字符串拼接
- 页面局部刷新返回
- 通用响应结构定义

P1 的核心目标不是先加功能，而是先把 `admin.rs` 从“大杂烩”拆成可维护结构，降低后续修改成本。

## 当前问题总结

### 1. 文件过大，职责混杂

`admin.rs` 已经同时包含：

- dashboard / providers / api keys / services / logs / settings 页面处理
- detail page 处理
- partial 列表处理
- headless admin API
- metrics / models 等管理接口
- 请求体与响应体定义
- 删除逻辑与联动删除逻辑
- 管理鉴权逻辑

结果是：

- 改一个小功能也要在超大文件里找位置
- UI 路由、API 路由、数据访问耦合在一起
- 很难在不引入回归的前提下继续迭代

### 2. SQL 和 HTML 生成散落在 handler 中

很多 handler 里同时做：

- 登录校验
- SQL 查询
- 结果整形
- HTML 拼接
- 返回响应

这会导致：

- 逻辑复用差
- 查询层无法单独测试
- HTML 渲染逻辑难以复用

### 3. UI 管理接口和 Headless API 混在一起

当前同一个文件里同时承载：

- 面向浏览器页面的 handler
- HTMX partial handler
- JSON admin API handler

虽然都属于 admin 域，但交互方式不同，拆分后会明显更清晰。

## P1 建议拆分结构

建议把 `src/app/admin.rs` 拆成如下结构：

```text
src/app/
  admin/
    mod.rs
    authz.rs
    dto.rs
    pages.rs
    partials.rs
    api.rs
    queries.rs
    mutations.rs
    render.rs
```

如果想更保守一点，也可以先拆成第一阶段结构：

```text
src/app/
  admin/
    mod.rs
    pages.rs
    api.rs
    data.rs
    authz.rs
```

推荐先用保守拆法落地，再逐步细分。

## 各模块职责建议

### `mod.rs`

职责：

- 暴露 admin 模块公共函数
- 统一 `pub(crate) use ...`
- 保持 `app.rs` 中路由注册改动尽量小

目标：

- 让外部继续通过 `admin::xxx` 访问 handler
- 内部实现可逐步迁移，不要求一次性改完所有调用点

### `authz.rs`

职责：

- 管理后台权限判断
- 抽出当前 `is_admin_authorized`
- 如有需要，补统一的 UI 登录校验辅助函数

建议包含：

- `is_admin_authorized`
- `ensure_ui_login` 或同类封装

这样做的好处是：

- 页面鉴权和 API 鉴权都能集中管理
- 后续切换认证策略时不会到处改

### `dto.rs`

职责：

- 放所有 admin 域的请求/响应结构体

建议迁移内容：

- `ApiResponse`
- `ServiceOut`
- `ProviderOut`
- `ApiKeyOut`
- `CreateServiceReq`
- `CreateProviderReq`
- `BindProviderReq`
- `CreateApiKeyReq`
- `DeleteApiKeyQuery`
- `DeleteServiceQuery`
- 各类 `sqlx::FromRow` 结构体

这样能把“数据定义”和“handler 流程”分开。

### `pages.rs`

职责：

- 页面级 handler
- detail page handler
- 页面入口渲染

建议迁移内容：

- `home`
- `admin_page`
- `admin_dashboard`
- `admin_providers`
- `admin_api_keys_page`
- `admin_services_page`
- `admin_logs_page`
- `admin_settings_page`
- `admin_provider_detail_page`
- `admin_service_detail_page`
- `admin_api_key_detail_page`

页面 handler 的共性是：

- 依赖 UI 登录
- 返回 `Html` / `Redirect`
- 通常组合模板和数据

### `partials.rs`

职责：

- HTMX 局部渲染相关 handler
- 列表刷新、局部 stats、局部 logs

建议迁移内容：

- `admin_stats_partial`
- `admin_providers_list_partial`
- `admin_api_keys_list_partial`
- `admin_services_list_partial`
- `admin_logs_list_partial`
- `admin_create_provider_partial`
- `admin_create_api_key_partial`
- `admin_providers_delete`
- `admin_api_keys_delete`
- `admin_services_delete`

这是因为这些接口本质上都是“UI 的局部交互动作”，和纯 JSON API 不同。

### `api.rs`

职责：

- Headless 管理 API
- 统一返回 JSON
- 统一走 `is_admin_authorized`

建议迁移内容：

- `api_list_services`
- `api_create_service`
- `api_list_providers`
- `api_create_provider`
- `api_bind_provider`
- `api_list_api_keys`
- `api_create_api_key`
- `health`
- `metrics`
- `models`

说明：

- `health / metrics / models` 也可以单独拆到 `system.rs`
- 但 P1 阶段不必拆太细，放 `api.rs` 也可以接受

### `queries.rs`

职责：

- 放所有只读 SQL 查询
- 让 handler 负责流程，查询函数负责数据读取

建议抽取：

- provider detail 查询
- service detail 查询
- api key detail 查询
- provider list 查询
- api key list 查询
- service list 查询
- logs 查询
- dashboard stats 查询

这样后续可以做到：

- handler 更短
- SQL 更集中
- 更容易发现重复查询

### `mutations.rs`

职责：

- 放所有写入型操作
- 尤其是带级联删除、副作用的逻辑

建议抽取：

- 创建 provider
- 创建 api key
- 绑定 provider 到 service
- 删除 provider
- 删除 api key
- 删除 service

特别是下面几类最值得先抽：

- `admin_create_api_key_partial`
- `admin_api_keys_delete`
- `admin_services_delete`
- `api_create_api_key`
- `api_bind_provider`

因为这些逻辑带有明显的数据联动和业务规则。

### `render.rs`

职责：

- 放 HTML 字符串拼接辅助函数
- 让 handler 不再直接堆大段 `format!`

建议抽取：

- provider rows 渲染
- service rows 渲染
- api key rows 渲染
- logs rows 渲染
- detail page 中的 table rows 渲染

这是 P1 里“可选但很值”的一步。

如果想控制改动范围，也可以先不拆 `render.rs`，等 `pages/partials` 稳定后再做。

## 建议的落地顺序

### 阶段 1：先做不改行为的物理拆分

目标：

- 不改变 SQL
- 不改变返回值
- 不改变路由
- 只搬代码位置

顺序建议：

1. 先建 `src/app/admin/` 目录和 `mod.rs`
2. 先把 `dto` 与 `authz` 拆出去
3. 再拆 `api.rs`
4. 再拆 `pages.rs`
5. 最后拆 `partials.rs`

原因：

- `dto` / `authz` 最独立，最容易先拆
- `api` 和 `pages` 逻辑清晰，适合优先分流
- `partials` 最杂，放最后处理更稳

### 阶段 2：抽查询与写入函数

当物理拆分完成后，再做：

- 提取 `queries.rs`
- 提取 `mutations.rs`
- 让 handler 变薄

这一步仍然不应追求“架构完美”，重点是：

- 让 handler 读起来像流程编排
- 让 SQL 从 handler 中退出来

### 阶段 3：可选的渲染抽象

如果后续发现：

- `pages.rs` / `partials.rs` 里仍然有大量 HTML 拼接
- 多个页面存在重复行渲染逻辑

再继续引入 `render.rs`。

这一步不一定要在 P1 一次做完。

## 路由层如何保持稳定

当前 `app.rs` 中大量路由直接引用 `admin::...`。

P1 最稳的做法是：

- 在 `admin/mod.rs` 中重新导出拆分后的 handler
- 尽量不修改 `app.rs` 的路由注册代码

例如：

- `pub(crate) use pages::admin_page;`
- `pub(crate) use partials::admin_services_list_partial;`
- `pub(crate) use api::api_create_api_key;`

这样拆分对外部调用点影响最小。

## 本轮拆分要避免的事情

P1 不建议同时做下面这些事：

- 不要顺手改 UI 文案
- 不要顺手重写 SQL
- 不要顺手改 service / key 业务模型
- 不要把 gateway 逻辑一起改掉
- 不要引入模板引擎替换现有字符串模板方案

原因很简单：

- P1 的目标是先拆结构，不是混合重构
- 一旦把“结构调整”和“行为调整”绑在一起，回归风险会明显升高

## 我建议优先抽出的高价值代码块

如果只做最小可行拆分，建议优先抽这几块：

### 第一优先级

- `is_admin_authorized` -> `authz.rs`
- 所有请求/响应 struct -> `dto.rs`
- 所有 `api_*` handler -> `api.rs`

### 第二优先级

- 所有页面入口 handler -> `pages.rs`
- 所有 partial handler -> `partials.rs`

### 第三优先级

- 删除相关逻辑 -> `mutations.rs`
- 列表查询 / 详情查询 -> `queries.rs`

## 预期收益

完成 P1 后，预期会得到这些收益：

- 新功能开发时更容易找到落点
- UI 页面逻辑和 API 逻辑不再缠在一起
- SQL 查询更容易集中治理
- 删除 / 创建等高风险逻辑更容易审查
- 为 P2 的 `gateway.rs` 公共流程抽象腾出精力

## 后续优化路径

在 P1 完成后，建议继续按下面顺序推进：

### P2

重构 `gateway.rs`，抽离 OpenAI / Anthropic 共用主流程：

- key 识别
- 配额校验
- 运行时限流
- provider 选择
- model mapping
- 上游调用
- 请求统计

### P3

把 `service` 从“有表”做成“有完整行为”：

- 真正启用 `routing_strategy`
- 支持权重
- 支持 fallback
- 支持 provider 健康状态
- 可逐步补 service 级限制能力

### P4

最后再补更偏生产级的能力：

- provider 密钥保护
- 更强的审计能力
- 多实例限流一致性
- 更严格的管理认证与安全策略

## 一句话总结

最优先的不是继续加功能，而是先把 `admin.rs` 拆成：

- 页面
- partial
- API
- 鉴权
- 数据结构
- 查询 / 写入

先把结构理顺，再做后续网关和 service 能力升级。

## 当前已完成进展

截至当前版本，这份方案中与 admin 模块拆分相关的核心步骤已经实际落地。

### 已完成：P1 物理拆分

原本巨大的 `src/app/admin.rs` 已被收敛为门面文件，当前主要通过 `pub(crate) use ...` 对外暴露 handler。

已拆出的模块包括：

- `authz.rs`
- `dto.rs`
- `api.rs`
- `pages.rs`
- `partials.rs`

并且：

- 外部路由调用方式保持不变
- `app.rs` 中路由注册没有被大改
- 拆分阶段以“只搬代码、不改行为”为原则完成

### 已完成：P1.5 查询与写入下沉

admin 域的数据访问已从 handler 中明显抽离，新增：

- `queries.rs`
- `mutations.rs`

当前状态：

- `pages.rs` 主要负责页面流程编排
- `partials.rs` 主要负责 HTMX 交互流程编排
- `api.rs` 主要负责 JSON admin API 流程编排
- 查询集中到 `queries.rs`
- 写入集中到 `mutations.rs`

已经下沉的典型内容包括：

- detail 查询
- list 查询
- dashboard stats 查询
- metrics 查询
- create / delete / bind / upsert 等写入逻辑

### 已完成：渲染层抽取

原先散落在 `pages.rs` 和 `partials.rs` 中的大段 HTML 字符串拼接，已进一步抽离到：

- `render.rs`

目前已经抽出的渲染块包括：

- provider detail 中的 service rows
- service detail 中的 provider rows
- service detail 中的 api key rows
- API key 页面中的 provider options
- providers 列表 rows
- api keys 列表 rows
- services 列表 rows
- logs 列表 rows

这样当前 admin 层已经形成了比较清晰的分层：

- `authz.rs`：鉴权
- `dto.rs`：结构定义
- `api.rs`：JSON API handler
- `pages.rs`：页面 handler
- `partials.rs`：HTMX handler
- `queries.rs`：查询
- `mutations.rs`：写入
- `render.rs`：HTML 渲染
- `admin.rs`：门面导出

### 当前收益

与初始状态相比，当前已经得到这些实际收益：

- `admin.rs` 不再是超大单文件
- handler 明显变薄，职责更清楚
- SQL 查询和写入已集中治理
- HTML 拼接已集中治理
- 后续继续做 admin 功能时更容易定位落点
- 为后续抽公共鉴权/页面壳逻辑打下了基础

### 后续建议

admin 这一轮大拆分已经基本完成，后续建议优先做：

1. 补充少量公共辅助函数，继续减少 `pages.rs` / `partials.rs` 的重复样板
2. 视需要补 admin 层测试或最基本的回归验证
3. 再进入 `gateway.rs` 的公共流程抽象
