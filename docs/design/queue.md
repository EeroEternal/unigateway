# 并发防挤压与异步排队机制 (Concurrency & Queuing)

UniGateway 在 **`unigateway-config::runtime`** 中提供按 Gateway API Key 的 QPS、并发与排队语义；宿主 HTTP 进程在鉴权通过后应调用 `GatewayState` 的 acquire/release API，再进入 host/core 执行。下文描述的是该运行时模型的设计意图（历史上曾由已删除的 `src/middleware.rs` 在 Axum 层驱动）。

## 设计初衷

在直接与各大模型供应商（Provider）通信时，如果完全不加限制，很容易将并发压力直接透传给 Provider 侧，带来极其生硬的 `HTTP 429 Too Many Requests` 以及由此引发的下游报错风暴。

UniGateway 摒弃了僵硬的“达到限流立刻拒绝”（Fail-Fast）策略。当突发请求超过了 API Key 设置的 `concurrency_limit` 时，请求并不会立即被抛弃，而是会**排队挂起等待（Sleep & Retry）**一段时间。只要前面正在执行的连接释放了限额，挂起的请求就能够马上平滑递补。

## 核心工作原理

该机制基于 Rust 和 Tokio 的原子原语构建，通过以下方式实现低损耗的多维流控：

1. **统一追踪**：每个生效的 Gateway API Key，都会在内存的 `RuntimeRateState` 里同时维护三个指标：
   * `request_count`: 窗口内（比如 1 秒）累加的全局请求量，用于计算 QPS（并发限流同样依赖于此）。
   * `in_flight`: 当前真正在执行打向上游模型提供商的并发连接数。
   * `in_queue`: 当前正在排队，由于没拿到并发许可而挂起（sleep）等待中的请求数。
  
2. **异步自旋等待锁（Notify Loop）**：
   * 当新请求进来时，若发现 `in_flight` 小于 `concurrency_limit`，就会顺利直接抢到执行权，`in_flight +1`。
   * 如果 `in_flight >= concurrency_limit`，我们使用 [`tokio::sync::Notify`](https://docs.rs/tokio/latest/tokio/sync/struct.Notify.html) 来阻塞并挂起该调用线程。

3. **并发释放与击鼓传花**：
   * 当任何正在执行的连接完成或失败时，在释放资源 (`release_inflight`) 时，会扣减一次 `in_flight`。
   * 随后立即调用 `notify.notify_one()`。这就像一声发令枪，瞬间精准唤醒在后台等待通道里“睡得最香”的第一个后补请求。被唤醒的请求重新校验一次剩余容量，成功接替工作。

## 租户防霸占与熔断策略

为了保护网关内存不被某个失控脚本消耗殆尽，排队机制设置了如下两道硬性红线：

1. **单 Key 队列深度限制 (Queue Depth Limit)** 
   默认常量 `MAX_QUEUE_PER_KEY = 100`。任何租户如果自己排队的请求高达 100 个，再新进来的请求将丧失“排队资格”，连大门都不让进，直接返回 HTTP 429。这个策略确保了即使一个恶意调用者使用极大的压力，也不会把全局服务器内存撑爆，让健康的长尾租户永远有资源可用。

2. **自生自灭的时效期 (Queue Timeout)** 
   默认常量 `MAX_WAIT_TIMEOUT = 5 秒`。如果在排队通道里焦急等待超过 5 秒仍然没有拿到请求限流的许可（意味着前面压迫的耗时大模型一直在卡住通道），系统会自动中断，返回 HTTP 429 并将资源让出。这同时也防止了客户端主动引发连接超时后的僵尸占坑。
