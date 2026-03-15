# UniGateway 与 OpenClaw 集成示例

这份文档给出一个面向个人开发者场景的 OpenClaw 集成示例，目标是让 OpenClaw 把 UniGateway 当成统一的本地 OpenAI-compatible 入口。

## 1. 集成思路

在当前实现中，OpenClaw 通过自定义 provider 接入 UniGateway：

- OpenClaw 负责 agent 交互与工具使用
- UniGateway 负责本地统一入口、mode 路由、fallback 和 provider 切换
- mode 的选择主要由不同的 gateway API key 决定

也就是说，OpenClaw 不需要直接理解 OpenAI、Anthropic、DeepSeek、Groq 等上游差异，只需要连到 UniGateway。

## 2. 前提条件

先确保你已经完成下面几步：

1. 已启动 UniGateway

```bash
ug serve
```

2. 已准备至少一个 mode 对应的 gateway key

如果已经执行过 `ug quickstart`，通常会自动生成 `default` mode 和对应 key。

你也可以手动创建：

```bash
ug create-api-key --key ugk_default_example --service-id default
```

3. 确认当前 mode 的接入模板

```bash
ug integrations --mode default --tool openclaw
```

## 3. 配置示例

在 `~/.openclaw/openclaw.json` 中加入：

```js
{
  agents: {
    defaults: {
      model: { primary: "unigateway/deepseek-chat" }
    }
  },
  models: {
    mode: "merge",
    providers: {
      unigateway: {
        baseUrl: "http://127.0.0.1:3210/v1",
        apiKey: "${UNIGATEWAY_API_KEY}",
        api: "openai-completions",
        models: [
          { id: "deepseek-chat", name: "UniGateway deepseek-chat" }
        ]
      }
    }
  }
}
```

然后导出环境变量：
```bash
export UNIGATEWAY_API_KEY=ugk_default_example
```

这时 OpenClaw 会通过 UniGateway 调用。

## 4. 更多进阶配置

如果你希望在 OpenClaw 中配置更多模型或别名，可以向 `providers` 中继续添加 `models` 字段关联：

```js
{
  agents: {
    defaults: {
      model: { primary: "ug-fast/deepseek-chat" }
    }
  },
  models: {
      "unigateway": {
        baseUrl: "http://127.0.0.1:3210/v1",
        apiKey: "${UNIGATEWAY_API_KEY}",
        api: "openai-completions",
        models: [
          { id: "deepseek-chat", name: "UniGateway Chat" },
          { id: "gpt-4o", name: "UniGateway Reasoning" }
        ]
      }
    }
  }
}
```

对应环境变量：

```bash
export UNIGATEWAY_API_KEY=ugk_default_example
```

## 5. mode、model 和 key 的关系

当前最重要的关系是：
- **gateway API key** 决定请求使用的 mode
- **OpenClaw model id** 决定想要使用的模型名
- **UniGateway** 决定这个模型名最终映射到哪个 upstream/provider

因此：

- 想切不同的接入配置，通常切的是 key
- 想切同一个 key 下的模型，通常切的是 model id

## 6. 验证步骤

建议按下面顺序验证：

1. 启动网关

```bash
ug serve
```

2. 检查 mode

```bash
ug mode list
ug route explain default
```

3. 检查当前 mode 的 OpenClaw 模板

```bash
ug integrations --mode default --tool openclaw
```

4. 跑诊断

```bash
ug doctor --mode default
ug test --mode default
```

5. 再从 OpenClaw 发起请求

如果 OpenClaw 能正常收到回复，说明接入链路已经打通。

## 7. 当前限制与建议

当前 OpenClaw 集成已经适合个人开发者场景，但还属于第一版模板支持：

- 还没有进一步做 OpenClaw 专用的高级回退配置或更细的错误诊断

当前建议是：

- 先以 OpenAI-compatible provider 方式稳定接入
- 保证 `default` mode 下的主备链路跑通
- 再根据 OpenClaw 的实际使用体验继续细化模板
