# UniGateway `/v1/models` 注册表读取层设计

## 背景与目标

宿主 gateway 需要实现 OpenAI 兼容的 `GET /v1/models` / `GET /v1/models/{id}`。当前 UniGateway 只暴露分散的私有 helper，导致每个宿主都要重写：

- 解析 `ProviderEntry::model_mapping` + `default_model`
- 构造 `provider/model` 复合 id 与裸 alias
- 跨 provider 去重
- 只读鉴权（不计配额/限流）
- OpenAI `model` 对象 JSON

本设计把上述能力下沉到 `unigateway-config` / `unigateway-protocol`，让宿主只做透明转发与 HTTP 路由。

**明确不做：**
- 不在可嵌入 SDK 里内置 HTTP `/v1/models` 路由（router 归宿主）。
- 不在库里实时回调上游 provider 的 `/v1/models`（按需、归上层）。

## 范围

| 优先级 | 内容 | 落地 |
| --- | --- | --- |
| P0 | `ServiceModel` + `GatewayState::list_service_models` | `unigateway-config` |
| P1 | `routing_ids_for` / `ServiceModel::routing_ids` | `unigateway-config` |
| P2 | 确定性的排序与去重语义 | `unigateway-config` |
| P3 | `GatewayState::authorize_readonly` | `unigateway-config` |
| P4 | `openai_model_object` helper | `unigateway-protocol` |
| — | SDK 命名空间 re-export | `unigateway-sdk` |

## 新增公开 API

### `unigateway-config`

```rust
/// `/v1/models` 返回的单个模型条目。
#[derive(Debug, Clone)]
pub struct ServiceModel {
    /// 主路由 id，复合形 `provider/alias`。
    pub id: String,
    /// 裸 alias（来自 model_mapping key 或 default_model）。
    pub alias: String,
    /// 上游真实模型名（model_mapping value），default_model 无映射时为 None。
    pub canonical: Option<String>,
    /// 归属 provider 名，对应 OpenAI `owned_by`。
    pub owned_by: String,
}

impl ServiceModel {
    /// 返回路由可接受的所有 id 形状：`["provider/alias", "alias"]`。
    pub fn routing_ids(&self) -> Vec<&str>;
}

/// 构造路由可接受的 id 形状。顺序：复合形在前，裸 alias 在后。
pub fn routing_ids_for(provider: &str, alias: &str) -> Vec<String>;

/// 只读鉴权错误。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthError {
    InvalidKey,
    InactiveKey,
}

impl GatewayState {
    /// 某 service 下所有已绑定、已启用 provider 暴露的模型目录。
    pub async fn list_service_models(&self, service_id: &str) -> Vec<ServiceModel>;

    /// 扁平、已展开、已去重的 `(id, owned_by)` 列表，可直接构造 `/v1/models` 响应。
    pub async fn list_service_model_ids(&self, service_id: &str) -> Vec<(String, String)>;

    /// 校验 api-key 有效且 active，不消耗配额、不占用运行时限额。
    pub async fn authorize_readonly(&self, raw_key: &str) -> Result<GatewayApiKey, AuthError>;
}
```

### `unigateway-protocol`

```rust
/// 构造 OpenAI `model` 对象。
pub fn openai_model_object(id: &str, owned_by: &str) -> serde_json::Value;
```

输出形状：

```json
{
  "id": "...",
  "object": "model",
  "created": 0,
  "owned_by": "..."
}
```

### `unigateway-sdk`

```rust
#[cfg(feature = "config")]
pub use unigateway_config as config;
```

根命名空间不平铺新类型，避免膨胀；消费者通过 `sdk::config::ServiceModel` 等访问。

## 语义约定

### `list_service_models`

1. 通过 `select_all_providers_for_service(service_id, "")` 取绑定且启用的 provider，顺序 = binding priority 升序。
2. 每个 provider：
   - 尝试解析 `model_mapping` 为 `BTreeMap<String, String>`；解析失败或不是对象时，aliases 视为空。
   - 若 `default_model` 非空且未在已解析 aliases 中出现，将其追加到 aliases 末尾。
   - 每个 alias 构造 `ServiceModel`：
     - `id = "{provider}/{alias}"`
     - `canonical = model_mapping.get(alias).cloned()`（来自 default_model 则为 `None`）
     - `owned_by = provider.name`
3. 不在本层跨 provider 去重，保留结构化原始数据。

### `list_service_model_ids`

1. 基于 `list_service_models` 的结果。
2. 按 provider 顺序、provider 内 alias 顺序展开每个 `ServiceModel::routing_ids()`。
3. 用最终 `id` 字符串全局去重，先出现者保留。
4. 返回 `(id, owned_by)`，其中 `owned_by` 取产生该 id 的 `ServiceModel.owned_by`。

### `authorize_readonly`

1. 调用 `find_gateway_api_key(raw_key)`。
2. 不存在返回 `AuthError::InvalidKey`；存在但 `is_active != 1` 返回 `AuthError::InactiveKey`。
3. 否则返回 `Ok(key)`。
4. 全程不写 `used_quota`，不触发 `acquire_runtime_limit`。

零消耗语义依据：
- `used_quota` 的唯一写入点是 `increment_used_quota`（`select.rs`），只在 LLM 派发路径显式调用。
- `find_gateway_api_key` 与 `select_all_providers_for_service` 只读配置（`read_config`）。
- 限流/并发由 `acquire_runtime_limit` 显式 opt-in。

## 实现位置

| 能力 | 文件 | 说明 |
| --- | --- | --- |
| `ServiceModel`、`routing_ids_for`、`AuthError` | `unigateway-config/src/schema.rs` | 与现有配置类型同层 |
| `list_service_models`、`list_service_model_ids`、`authorize_readonly` | `unigateway-config/src/admin.rs` | 与现有 `GatewayState` 管理方法同层 |
| `openai_model_object` | `unigateway-protocol/src/responses/render.rs` | 与 response renderer 同层 |
| SDK re-export | `unigateway-sdk/src/lib.rs` | feature-gated |

## 测试计划

| 测试 | 位置 | 验证点 |
| --- | --- | --- |
| `routing_ids_for` 输出顺序 | `unigateway-config/src/schema.rs` tests | 复合形在前、裸 alias 在后 |
| `list_service_models` 基础形状 | `unigateway-config/src/admin.rs` tests | provider 顺序、alias 顺序、canonical、owned_by |
| `list_service_models` malformed mapping 容错 | `unigateway-config/src/admin.rs` tests | mapping 坏了仍含 default_model，不 panic |
| `list_service_model_ids` 去重 | `unigateway-config/src/admin.rs` tests | composite 去重、bare alias 跨 provider 去重、最终 id 唯一 |
| `authorize_readonly` 零消耗 | `unigateway-config/src/admin.rs` tests | 调用前后 `used_quota` 不变 |
| `authorize_readonly` 错误分支 | `unigateway-config/src/admin.rs` tests | InvalidKey / InactiveKey |
| `openai_model_object` 形状 | `unigateway-protocol/src/responses/tests.rs` | JSON 字段与值 |

## 宿主迁移路径

1. 上游 crate 发布新版（2.5.0 或后续）。
2. 宿主保留现有 `service_model_entries` / `authorize_gateway_readonly` / `openai_model_object` 兜底。
3. 升级后机械替换：
   - `service_model_entries(...)` → `state.list_service_model_ids(service_id).await`
   - `authorize_gateway_readonly(...)` → `state.authorize_readonly(raw_key).await`
   - 手搓 JSON → `unigateway_protocol::openai_model_object(...)`
4. 移除宿主临时实现。

## 兼容性

- 新增 API，无现有 API 变更。
- `ServiceModel` / `AuthError` / `routing_ids_for` 是新类型，不影响旧代码。
- SDK 新增 `config` feature：`config = ["dep:unigateway-config"]`。
- `unigateway-sdk/Cargo.toml` 增加 `unigateway-config` 依赖与 `config` feature。
- `unigateway-sdk/src/lib.rs` 增加 `#[cfg(feature = "config")] pub use unigateway_config as config;`。
- 默认 feature（`host`）不强制启用 `config`，保持可选。
