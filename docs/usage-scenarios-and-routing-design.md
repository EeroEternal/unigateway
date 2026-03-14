## UniGateway 场景驱动的路由与适配设计

本文从常见使用场景出发，反向约束 UniGateway 的抽象与实现优先级，帮助后续在“保持轻量”的前提下，把网关的表达力补足。

### 一、基础概念回顾（结合现有实现）

Service：下游感知到的“逻辑服务”，一个 Service 可以绑定多个 Provider，是路由与统计的基本单位，当前有 `routing_strategy` 字段，默认 `round_robin`。

Provider：具体的上游调用配置，包含 provider 类型（openai、anthropic 等）、endpoint_id、base_url、api_key、model_mapping 等。一个 Provider 可以被绑定到多个 Service。

API Key（网关 Key）：下游访问网关的凭证，与 Service 绑定，可以配置 quota_limit、qps_limit、concurrency_limit，用于租户级控制。

Model Mapping：从下游请求的 model 名，映射到上游 Provider 实际支持的 model 名，当前支持 JSON 映射或简单字符串直写。

路由主流程（已实现）：根据 API Key 找到 Service，再从被绑定的 Providers 中选择一个 Provider，当前使用轮询算法，并在调用前后记录 request_stats 和 request_logs。

### 二、场景一：单 Provider 直连场景

典型用户心智：只是想把 OpenAI（或某一个 Provider）“藏在网关后面”，自己只用记一个统一的网关地址和一个网关 API Key。

目标体验：
用户创建一个 Service 和一个 Provider，绑定二者，再创建一个 API Key；之后只要用这个 Key 调用 `/v1/chat/completions`，请求就稳定走这个 Provider，不关心后面的映射细节。

当前抽象与差距：
1. 抽象上已经满足：一个 Service 只绑定一个 Provider，本质上就是单 Provider 直连。
2. 需要补充的是“极简上手路径”和“显式推荐做法”，包括：
   a) 文档中的推荐流程：通过 CLI 或 JSON 管理接口创建 Provider 和 Service → 绑定 → 创建 API Key → 拿 Key 直接调 OpenAI 兼容接口。
   b) CLI 的一条龙脚本化体验：一条命令完成创建 Service、Provider、绑定与 Key，方便运维或 AI 应用直接集成到初始化脚本。
3. 技术实现层面的改动很小，主要是文档和 UX 引导，可以优先落地。

### 三、场景二：多 Provider 轮询场景

典型用户心智：我有多个同类 Provider，希望做简单的“多活”与“分摊流量”，不需要复杂算法，轮询就够用。

目标体验：
为同一个 Service 绑定多个 Provider，routing_strategy 使用默认值即可；请求自然被轮询分配到不同 Provider，且不会出现明显的热点或单点。

当前抽象与实现：
1. 数据层：
   service_providers 表已经支持一个 Service 绑定多个 Provider，providers 表也有 `weight` 字段。
2. 运行时：
   `select_provider_for_service` 中已经实现了简单的轮询逻辑，通过 service_id + protocol 作为 bucket 做 in-memory 轮转。
3. 差距主要在两个方面：
   a) 文档和 CLI/配置层还没有把这一能力作为“一等公民场景”明确出来，用户不知道“多绑定几个 Provider 就可以自动轮询”。
   b) weight 字段尚未真正参与路由策略。

建议的轻量优化：
第一步，保留当前轮询实现，重点在文档中明确写出：
同一个 Service 绑定多个 Provider 后，默认 routing_strategy=round_robin，会在这些 Provider 之间做轮询分发，适合简单多活与流量平摊。

第二步，再考虑引入最小成本的权重支持：
在不引入复杂算法组件的前提下，可以基于当前 providers.weight 做简单的“重复展开”或“加权轮询”。

### 四、场景三：多 Provider 权重场景

典型用户心智：有两个 Provider，一个便宜一个贵，希望大部分流量走便宜的，少量走贵的，但仍然是自动路由，不想在业务里写策略。

目标体验：
在 Admin UI 或 CLI 中为 Provider 设置 weight，比如 A=3、B=1，然后同一个 Service 绑定这两个 Provider，routing_strategy 仍为 round_robin；网关内部以 3:1 的比例将请求派发给 A 和 B。

抽象设计建议：
1. 沿用已有字段：
   直接使用 providers.weight 作为“相对权重”，不引入新的配置项。
2. 运行时算法（保持轻量）：
   可以在 select_provider_for_service 中调整为“加权轮询”，采用简单易实现的方式，例如：
   按照权重将 providers 展开为一个逻辑数组，在内存中做轮询游标。
   或使用经典的加权轮询算法，避免数组过大。
3. 行为约定：
   如果所有 Provider 的 weight 都为 NULL 或 0，则退化为平均轮询。
   如果有正权重，则按权重分配；错误配置（全为 0）时可以直接退回平均轮询。

从轻量角度看，只要路由策略仍然是“内存级简单算法 + SQLite 源数据”，就不会显著增加部署和维护成本。

### 五、场景四：简单 Fallback 场景

典型用户心智：我有主 Provider 和备 Provider，当主 Provider 挂掉或出现持续错误时，希望自动切到备 Provider；不需要复杂健康检查，只要粗粒度 fallback。

目标体验：
用户为 Service 配置多个 Provider，其中一个被标记为 primary，其他为 fallback；当 primary 一段时间内连续报错时，后续请求自动优先用 fallback。

抽象设计建议（兼顾轻量）：
1. 配置层：
   可以复用 weight 做“软优先级”，例如 primary 设置较大权重，fallback 设置较小权重，再配合“临时降权”实现简单 fallback。
   也可以引入一个非常轻量的新字段，如 providers.is_primary 或 providers.priority，用来区分主备。
2. 状态与算法：
   不引入复杂的健康检查系统，而是在网关进程内维护一个简单的“错误计数窗口”，例如：
   对每个 Provider 维护最近 N 次调用中的错误数或连续错误数，超过阈值后临时把该 Provider 排除出候选集合若干秒。
   状态可存在内存 HashMap 中，不必入库，重启后重新学习即可。
3. 行为约定：
   错误类型仅以 5xx 或连接错误为主，4xx 不算 Provider 故障。
   当所有 Provider 都被标记为不可用时，回退到原始策略（例如仍然尝试 primary），并返回明确的错误信息提醒用户检查配置。

这样的 fallback 方案不会引入额外外部依赖，又能覆盖绝大多数“主备”需求。

### 六、场景五：Embeddings 场景

典型用户心智：业务除了聊天，还需要 embeddings 能力，希望同样通过网关走一个统一的接口，不想分别对接 OpenAI/其他 Provider。

目标体验：
对下游暴露一个类似 `/v1/embeddings` 的 OpenAI 兼容接口，调用方式与官方 API 尽量一致；后端可根据 Service 和 Provider 路由到不同上游实现。

抽象与实现建议：
1. 协议适配层：
   在 protocol.rs 中新增 embeddings 相关的转换函数，比如：
   openai_embeddings_payload_to_request
   embeddings_response_to_openai_json
   并复用 llm-connector 中 embeddings 能力（如果已有），否则可以按当前 Chat 的模式扩展。
2. 路由逻辑：
   尽量与 chat 保持一致：同样通过 API Key → Service → Providers → select_provider_for_service 的流程选择上游，并复用 model_mapping。
3. 管理侧：
   初期可以不引入专门的 Embeddings UI，只在 Provider 的说明和 model_mapping 里标明该 Provider 支持哪些 embeddings 模型，通过 CLI 或 JSON 管理接口进行配置。

这样做增加的复杂度有限，却能覆盖非常常见的“聊天 + 检索”组合场景。

### 七、场景六：引入另一个主流聊天协议

典型用户心智：除 OpenAI/Anthropic 外，还想接入一个主流 LLM 服务商（如 Gemini、Groq、DeepSeek 等），但希望下游接口仍然是 OpenAI/Anthropic 兼容的。

目标体验：
通过 CLI 或 JSON 管理接口新增 Provider，选择新的 provider_type 和 endpoint_id；之后下游仍然只调用 `/v1/chat/completions` 或 `/v1/messages`，由网关根据 Service/Provider 配置将请求路由到对应家族的上游。

抽象与实现建议：
1. Provider 配置维度：
   已有的 provider_type + endpoint_id + base_url + api_key 足以描述大多数新上游，只需要在 llm_providers 库中维护好 endpoint 元数据。
2. 协议适配维度：
   如果 llm-connector 已经对该 Provider 做了统一封装，则在 protocol.rs 中无需额外适配。
   如果尚未封装，则可以在 llm-connector 层增加新的 LlmClient 构造方法和 ChatRequest/ChatResponse 适配，UniGateway 只需要选择正确的 endpoint 和 model。
3. 场景导向的文档：
   在 docs 中给一个非常具体的示例，比如“如何通过 UniGateway 使用 DeepSeek 或 Groq”，写清 Provider 配置、Service 绑定与下游调用参数。

从 UniGateway 的立场看，增加新的主流 Provider，主要是“llm_providers + llm-connector + provider 配置”的协同，网关自身路由抽象无需大改。

### 八、轻量级高价值场景补充（推荐优先级）

以下场景全部遵守“轻量级铁律”：不需要 Redis、不需要 Kubernetes、不需要额外服务，直接围绕现有 SQLite + 单进程网关能力展开。

#### 1) 个人开发者 / 本地多模型实验台（★★★★★）

场景描述：
一个人本地开发 AI 应用，希望同时试 OpenAI、DeepSeek、Anthropic，甚至本地 Ollama（custom backend），并且随时切换而不改业务代码。

为什么适合 UniGateway：
通过一次 quickstart 把多个 provider 绑定到同一个 service，下游只对接一个网关地址与一个网关 key；后续只调整 `routing_strategy` 为 `PriorityFallback` 或 `Single`，就能快速切换到更快或更稳的上游。

对应优化点：
1. v0.2.0 的 `PriorityFallback` + `Single` 策略。
2. 增加 `/v1/embeddings` 后，直接覆盖“本地向量实验 + 聊天”组合。
3. quickstart 推荐示例：
   `unigateway quickstart --provider deepseek --key sk-xxx --fallback openai`

落地方式：
README 增加“本地实验 3 分钟跑通”小节，并给出完整 curl 示例。

#### 2) 小团队内部 AI 助手网关（★★★★☆）

场景描述：
3-8 人团队共用一个后端 AI 服务（如 ChatGPT + Claude），需要按人或按项目做限流、配额和统一日志统计。

为什么适合 UniGateway：
内置 Admin UI + SQLite 日志 + 每 key 独立配额控制，部署体积和运维复杂度明显低于需要额外运行时环境的重方案。

对应优化点：
1. `Weighted` / `RoundRobin` 路由用于团队级流量分配。
2. 统一 JSON 错误返回（如配额超限时直接提示“已达本月配额”）。
3. quickstart 增加 `--team` 模式，一键创建多个 API Key（例如 3 个）。

落地方式：
提供 `.env.example` 与“团队共享网关”配置模板，支持复制即用。

#### 3) RAG / 知识库生产小程序（★★★★，建议与 embeddings 同步）

场景描述：
文档问答、代码助手、客服机器人等场景同时依赖 chat 与 embeddings，但不希望维护两套上游 API 接入逻辑。

为什么适合 UniGateway：
同一个网关统一提供 `/v1/chat/completions` 与 `/v1/embeddings`，并在路由层按模型类别分流：embeddings 固定走低成本 provider，chat 走高质量 provider。

对应优化点：
1. 尽快补齐 embeddings endpoint（可复用现有 chat 路径的大部分结构）。
2. 路由策略支持 `model_pattern`（例如 `gpt-4o -> OpenAI`，`text-embedding-* -> DeepSeek`）。

落地方式：
在文档与 README 同时提供“RAG 最小可用配置”示例，突出“一套网关双接口”。

#### 4) 成本控制型生产代理（中小型 SaaS 后端）

场景描述：
日调用量达到万级后，希望将大部分流量自动分配到低成本模型，复杂请求再走高质量高成本模型。

为什么适合 UniGateway：
仅靠 `Weighted` + `PriorityFallback` 就可以实现“成本优先 + 质量兜底”的基础策略，不需要在业务代码实现智能路由。

对应优化点：
1. v0.2.0 先落地基于权重的稳定分流，满足 80% 成本优化需求。
2. v0.3.0 再引入基于现有 logs 的轻量 latency scoring，作为增强项而非依赖项。

落地方式：
增加一个“生产成本控制模板”：默认 70% 低成本模型、30% 高质量模型，并附带可直接应用的 service/provider 配置片段。

### 九、后续抽象与实现优先级建议

在保持轻量的前提下，可以按以下顺序推进：
第一步：完善“单 Provider 直连”和“多 Provider 轮询”场景的文档与 CLI 引导，不必先改代码，先让现有能力“显性化、易发现、易上手”。

第二步：在 select_provider_for_service 中实现最小版本的权重支持，真正利用上 providers.weight，补全多 Provider 权重路由场景。

第三步：在 gateway.rs 内增加一层极简的 Provider 错误计数逻辑，实现“软 fallback”，只在当前进程内生效，不做复杂持久化。

第四步：扩展 embeddings 路径和一个额外主流 Provider 的示例，将“聊天 + embeddings + 多 Provider 路由”打通，作为 UniGateway 的主力卖点场景组合。

这份文档的目标不是把网关做重，而是用有限的几个高频场景，把现有抽象（Service / Provider / API Key / Model Mapping / Routing Strategy）用满、用好，并为后续的迭代提供清晰的方向锚点。

