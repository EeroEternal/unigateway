# Public API 强类型化研究

本文聚焦 UniGateway 当前 public request API 的强类型化路径。当前路线已经从“小步演进”推进到 `Message` 的 block-first 结构升级：协议语义先收口为 typed helper，随后 `Message.content` 升级为 `Vec<ContentBlock>`。

## 1. 当前代码事实

截至当前版本，chat public API 有三个值得分开的层次：

### 1.1 稳定公开形状

- `Message` 已是 block-first 消息：`role + content: Vec<ContentBlock>`。
- `ProxyChatRequest` / `ProxyResponsesRequest` / `ProxyEmbeddingsRequest` 仍是 embedders 直接构造的 public struct。
- `ContentBlock` 已经公开，`StructuredMessage` 现在是 `Message` 的兼容别名；文本场景可用 `Message::text(role, text)`。

### 1.2 已存在但偏弱类型的协议语义

当前有几类语义不是靠字段表达，而是通过 `metadata: HashMap<String, String>` 和字符串常量传递：

- `unigateway.client_protocol`
- `unigateway.openai_raw_messages`
- `unigateway.thinking_signature_status`
- `unigateway.requested_model_alias`

这些值由 `unigateway-protocol` 请求解析入口写入，再由 `unigateway-core` driver 和 `unigateway-protocol` 响应渲染读取。当前生产路径已经通过 typed helper 收口主要读写点，常量仍保留用于兼容和测试夹具。

### 1.3 一个关键约束

`request.metadata` 不是纯内部通道。它还是 embedder 的公开标签面，并且会并入 `RequestReport.metadata`，用于 hook、审计和 tracing。

这意味着：

- 不能把它简单视为内部状态容器。
- 内部协议控制 key 与用户自定义标签混在同一个 public map 里，本身就是一种 API 泄漏。
- 下一步如果要做更强类型化，首要目标应该是“把内部语义从用户 metadata 面里抽出来”，而不是先改 `Message`。

## 2. 当前真正的问题

目前的问题不在于 `Message` 太弱，而在于“协议来源”和“signature 状态”已经是跨模块决策语义，却仍靠字符串判断。

具体表现：

- 请求入口通过写 metadata 决定后续转换路径。
- driver 曾通过 `== Some("openai_chat")` 之类的字符串比较决定上游构造策略；现在已收敛到 `ClientProtocol` helper。
- thinking placeholder / real signature / absent signature 的语义已经进入 `ThinkingSignatureStatus` helper，并由请求 parser 写入。
- 这些内部 key 还会自然流入 `RequestReport.metadata`，让 hook 观察面混入 protocol plumbing 细节。

换句话说，当前最值得先强类型化的不是 message content，而是 request semantics。

## 3. 哪些该升级，哪些先不要升级

### 3.1 适合先升级成 public typed API 的语义

以下语义已经满足“跨边界、跨模块、具备稳定含义”的条件：

- `ClientProtocol`
  - 值域已经比较稳定：`openai_chat`、`anthropic_messages`、`neutral`。
  - 会影响 driver 的转换分支与协议渲染策略。

- `ThinkingSignatureStatus`
  - 适合表达 `absent`、`placeholder`、`verbatim` 这类不可混淆状态。
  - 它比裸 `Option<String>` 更能区分“没有 signature”和“有占位 signature 但不可续用”。

### 3.2 暂时不值得升格为 public typed field 的语义

以下信息更适合作为内部传递细节，至少当前不应急于升格：

- `requested_model_alias`
  - 它主要用于 Anthropic 渲染路径恢复客户端请求时的 model 名称。
  - 这是 renderer-local 的兼容语义，不一定是 embedder 需要长期感知的一等 request 字段。

- `openai_raw_messages`
  - 这更像一个实现细节或派生事实。
  - 一旦 `ClientProtocol` 与 typed accessor 存在，它可以收敛为内部 helper，而不必继续作为 public metadata 约定扩散。

## 4. 为什么现在不该直接给 `ProxyChatRequest` 加字段

直觉上的做法是给 `ProxyChatRequest` 新增：

```rust
pub client_protocol: ClientProtocol
pub thinking_signature_status: ThinkingSignatureStatus
```

但这在当前 public API 上是破坏性的。

原因很直接：`ProxyChatRequest` 是 public struct，embedders 普遍用 struct literal 构造。给它新增字段会打断现有初始化代码，即使逻辑上只是“加默认值”。

同理，直接把 `metadata` 从 `HashMap<String, String>` 改成新的 wrapper 类型也会是破坏性变更。

因此，第一阶段没有通过“修改现有 public struct 字段”来实现强类型化，而是先引入 typed helper。当前 `Message.content` 的升级已经进入 breaking API 范畴，后续发布说明需要明确标注迁移方式。

## 5. 推荐的小步路线

### 阶段 A：先提供 typed view，不改现有 struct 形状

这是当前最合适的一步。

建议新增：

- `ClientProtocol` enum
- `ThinkingSignatureStatus` enum
- `ChatRequestSemantics` 或 `ProxyChatRequestSemantics` 这类只读视图类型
- `ProxyChatRequest` 上的解析/写回 helper，或同模块 free functions

典型能力：

- 从现有 metadata 解析出 typed semantics。
- 用统一 helper 写回 metadata，避免散落的裸字符串比较。
- driver / protocol 层只依赖 typed accessor，不直接读写 magic string。

这一阶段的收益：

- 不破坏 `Message`。
- 不破坏 `ProxyChatRequest` struct literal。
- 能把字符串常量和分支逻辑收敛到单点。
- 为后续真正的 typed field 升级铺路。

### 阶段 B：如果 typed semantics 继续增长，再引入 companion 类型

当 request semantics 不再只有两三个字段时，再考虑引入新的 companion API，而不是继续往 metadata key 上堆语义。

可选形态：

- `ProxyChatRequestBuilder`
- `ChatRequestEnvelope`
- `ProxyChatRequestExt` / `TypedProxyChatRequest`

这个阶段的重点仍然不是替换 `Message`，而是把“协议来源、native payload 保留策略、signature 语义”从字符串 metadata 提升到有名字的抽象上。

### 阶段 C：下一个 breaking release 再考虑真正字段升级

只有在以下条件同时成立时，才建议做 public struct 级别的升级：

- typed semantics 已经稳定，不再只是探索。
- embedder 侧确实需要显式构造这些字段，而不只是消费它们。
- metadata key 方案已经明显影响可维护性。
- 准备接受一次 semver-breaking release。

这一阶段已经采用的方向是：

- 保持 `ProxyChatRequest` 字段形状不变，避免同时引入 request envelope 和 message breaking change。
- 把 `client_protocol`、`signature_state` 之类先放进 typed metadata helper。
- 将 `Message` 本身升级为 `Message { role, content: Vec<ContentBlock> }`。
- 保留 `Message::text()` 作为文本迁移入口，保留 `StructuredMessage` 作为兼容别名。

## 6. 关于 `Message` 的当前状态

`Message` 已经完成 block-first 升级。现在的推荐用法是：

```rust
Message::text(MessageRole::User, "hello")

Message::from_blocks(
  MessageRole::Assistant,
  vec![
    ContentBlock::Thinking { thinking, signature },
    ContentBlock::Text { text },
  ],
)
```

升级后的收益：

- Anthropic thinking signature 可以作为 typed block 保留。
- OpenAI tool call / Anthropic tool_use 可以在中立消息里表达。
- fallback driver 路径不再只能依赖纯文本 `content`。

需要在发布说明中明确的 breaking 点：旧的 `Message { role, content: "...".to_string() }` 应迁移为 `Message::text(role, "...")`，结构化内容应迁移为 `Message::from_blocks(role, blocks)`。

## 7. 近期可执行项

这一轮已经落地的事项：

1. 新增 `ClientProtocol` 与 `ThinkingSignatureStatus` enum。
2. 新增从 `ProxyChatRequest.metadata` 解析 typed semantics 的 helper。
3. 把 `unigateway-protocol` 请求入口和 `unigateway-core` driver 的字符串判断收敛到 helper。
4. 为“typed helper 与旧 metadata 兼容”补保护性测试。
5. 为 Anthropic requested model alias 增加 helper，响应渲染和 host dispatch 不再直接读裸 key。
6. 新增 `StructuredMessage`，随后将 `Message` 本身升级为 block-first 内容结构。

接下来如果继续推进，应优先考虑：

1. 新增 request envelope / builder，让 embedders 构造 chat request 时不必直接操作 `metadata`。
2. 评估是否需要把内部 protocol control 从 `RequestReport.metadata` 中剥离出来。
3. 为 `Message` breaking change 补 changelog / migration note。

这一步完成后，UniGateway 就已经迈出了“public API 更强类型化”的第一步，而且没有提前引爆 `Message` 或 public request struct 的破坏性调整。

## 8. 进入下一阶段的信号

当出现下列任一情况时，可以考虑从阶段 A 进入阶段 B 或 C：

- `client_protocol` 之外又新增多个 protocol-level 决策字段。
- `thinking_signature_status` 需要在 public hook / host API 中被稳定暴露。
- `metadata` 内部 key 已经开始影响 embedder 对 report metadata 的理解。
- 宿主应用需要显式构造“可续轮 Anthropic assistant message”的 typed request surface，并且仅靠 `ProxyChatRequest` struct literal 已经不够好用。

下一步最稳妥的做法是：在 `Message` block-first 的基础上补 builder / envelope，继续把弱类型 protocol control 从 public metadata 面里剥离。