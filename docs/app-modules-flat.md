# App 目录按功能扁平化说明

> **说明**：本文描述的是过渡期「app/ 下按功能拆分」的方案。当前已落地为 **src 根下完全扁平**：无 `app/` 目录，无 UI 模块（dashboard、logs、settings、auth、shell、render 已删），入口为 `main.rs`，详见 `refactor-summary.md` 与 `directory-structure.md`。

## 目标

去掉宽泛的 `admin` 目录，按**功能域**在 `app/` 下扁平组织：每个功能一块（service / provider / api_key / dashboard / logs / settings），再加少量共享层（authz、dto、queries、mutations、render、shell）和系统接口（system）。

## 新结构

```
src/app/
├── mod.rs              # 入口：run、AppConfig、路由注册
├── auth.rs             # 登录/登出（不变）
├── gateway.rs          # 网关 chat 入口（不变）
├── storage.rs          # DB 初始化与网关用查询（不变）
├── types.rs            # AppState、GatewayApiKey 等（不变）
│
├── authz.rs            # 管理侧鉴权（原 admin/authz）
├── dto.rs              # 管理侧请求/响应/Row 结构（原 admin/dto）
├── shell.rs            # 布局与登录校验辅助（原 admin/shell）
├── render.rs           # HTML 片段渲染（原 admin/render）
├── queries.rs          # 管理侧只读查询（原 admin/queries）
├── mutations.rs        # 管理侧写入（原 admin/mutations）
│
├── dashboard.rs        # 首页、Dashboard 页、stats 局部
├── provider.rs         # Provider 列表/详情/创建/删除（页 + 局部 + JSON API）
├── service.rs          # Service 列表/详情/删除（页 + 局部 + JSON API）
├── api_key.rs         # API Key 列表/详情/创建/删除（页 + 局部 + JSON API）
├── logs.rs             # 请求日志页与列表局部
├── settings.rs         # 设置页
└── system.rs           # health、metrics、models（无 UI）
```

## 模块职责

| 模块 | 职责 |
|------|------|
| **dashboard** | home（重定向）、admin_page、admin_dashboard、admin_stats_partial |
| **provider** | admin_providers、admin_provider_detail_page、admin_providers_list_partial、admin_create_provider_partial、admin_providers_delete、api_list_providers、api_create_provider、api_bind_provider |
| **service** | admin_services_page、admin_service_detail_page、admin_services_list_partial、admin_services_delete、api_list_services、api_create_service |
| **api_key** | admin_api_keys_page、admin_api_key_detail_page、admin_api_keys_list_partial、admin_create_api_key_partial、admin_api_keys_delete、api_list_api_keys、api_create_api_key |
| **logs** | admin_logs_page、admin_logs_list_partial |
| **settings** | admin_settings_page |
| **system** | health、metrics、models |

路由仍在 `app/mod.rs` 中统一注册，仅将原来的 `admin::xxx` 改为 `dashboard::xxx`、`provider::xxx` 等。
