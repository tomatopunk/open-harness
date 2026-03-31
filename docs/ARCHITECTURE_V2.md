# Open Harness 架构 V2 - 重构指南

## 概述

Open Harness V2 是一个真正开放的 agent 内核架构，参考了 oh-my-openagent 的插件化设计。

## 核心设计原则

1. **最小化内核** - 只保留 agent 循环、事件总线、插件管理
2. **一切皆插件** - 模型、工具、渠道、存储都作为插件加载
3. **MCP 优先** - 所有外部能力通过 MCP 提供
4. **配置驱动** - 参考 oh-my-openagent 的分层配置系统
5. **钩子系统** - 生命周期钩子支持扩展
6. **LLM 抽象** - 通用的 LLM provider 层，rig 只是其中一个实现

## 新的目录结构

```
open-harness/
├── apps/
│   └── kernel/              # 核心 kernel 二进制
├── crates/
│   # 核心 crate
│   ├── agent-kernel/        # 最小化 agent 内核
│   ├── plugin-system/       # 插件系统
│   ├── llm-providers/       # 通用 LLM provider 抽象（rig/openai/anthropic 等）
│   ├── mcp-bridge/          # 增强的 MCP 桥接
│   # 保留的核心 crate
│   ├── agent-ports/         # 端口抽象
│   ├── state-abstraction/   # 状态抽象
│   ├── storage-registry/    # 存储注册
│   └── mcp-client/          # MCP 客户端
├── plugins/                  # 插件目录
│   ├── gateway-plugin/       # API 网关插件
│   └── manage-plugin/        # 管理插件
└── skills/                   # 技能目录（MCP 服务器格式）
```

## 核心组件说明

### 1. Agent Kernel (`crates/agent-kernel`)

最小化 agent 内核，提供：

- 生命周期管理
- 事件总线
- 钩子系统
- 插件协调
- LLM provider 集成
- MCP bridge 集成

### 2. Plugin System (`crates/plugin-system`)

插件系统，提供：

- 插件 trait 定义
- 插件发现和加载
- 插件清单管理
- 插件上下文和共享状态

### 3. LLM Providers (`crates/llm-providers`)

通用的 LLM provider 抽象层，支持多种实现：

- **Rig Provider** (`rig.rs`) - 使用 rig 库
- **OpenAI Provider** (待实现) - 直接使用 OpenAI API
- **Anthropic Provider** (待实现) - 直接使用 Anthropic API
- 更多...

#### 关键特性

- `LLMProvider` trait - 主 provider trait
- `CompletionClient` trait - 简单文本补全
- `ChatAgent` trait - 多轮对话
- `create_provider()` - 工厂函数，根据配置创建 provider

#### 使用示例

```rust
use llm_providers::{create_provider, ProviderConfig, ProviderType};

// 创建配置
let config = ProviderConfig::new(ProviderType::Rig, "gpt-4")
    .with_api_key("sk-...");

// 创建 provider
let provider = create_provider(&config)?;

// 创建 chat agent
let mut agent = provider.chat_agent(&config)?;
agent.set_preamble("You are a helpful assistant.");

// 发送消息
let response = agent.prompt("Hello!").await?;
```

### 4. MCP Bridge (`crates/mcp-bridge`)

增强的 MCP 桥接，提供：

- MCP 服务器生命周期管理
- 技能 MCP 管理（参考 oh-my-openagent）
- 工具发现和聚合
- 动态加载/卸载 MCP 服务器

## 插件开发指南

### 创建一个简单插件

1. 在 `plugins/` 目录创建插件目录
2. 创建 `plugin.yaml` 清单
3. 实现 `Plugin` trait
4. 在配置中启用

### 插件清单示例 (`plugin.yaml`)

```yaml
name: my-plugin
version: 0.1.0
description: My awesome plugin
authors:
  - Your Name
type: generic
enabled: true
dependencies: []
```

### 插件类型

- `api` - API 服务插件
- `tool` - 工具插件
- `model` - 模型插件
- `storage` - 存储插件
- `channel` - 渠道插件
- `generic` - 通用插件

### 插件实现示例

```rust
use async_trait::async_trait;
use plugin_system::{BasePlugin, Plugin, PluginContext, PluginResult};

pub struct MyPlugin {
    base: BasePlugin,
}

#[async_trait]
impl Plugin for MyPlugin {
    fn manifest(&self) -> &PluginManifest {
        self.base.manifest()
    }
    
    fn state(&self) -> PluginState {
        self.base.state()
    }
    
    async fn load(&mut self, ctx: &PluginContext) -> PluginResult<()> {
        // 加载逻辑
        self.base.load(ctx).await
    }
    
    async fn start(&mut self, ctx: &PluginContext) -> PluginResult<()> {
        // 启动逻辑
        self.base.start(ctx).await
    }
    
    // ... 其他 trait 方法
}
```

## 新架构 vs 旧架构

| 方面 | V1 (旧) | V2 (新) |
|------|---------|---------|
| 架构 | 封闭的 4 服务架构 | 开放的 agent 内核 |
| 模型系统 | 自研 model-runtime | llm-providers 抽象层 |
| 模型实现 | 单一 | 多 provider (rig/openai/anthropic) |
| 技能系统 | 自研 skill-system | MCP 服务器 |
| 扩展性 | 有限 | 插件化，高度可扩展 |
| 社区生态 | 有限 | 可集成多种 LLM 库 |

## 关于"Open"

Open Harness V2 真正做到了"Open"：

1. **开放架构** - 最小化内核，一切可扩展
2. **开放生态** - 支持多种 LLM provider（不只是 rig）
3. **开放协议** - MCP 优先，标准协议
4. **开放贡献** - 插件化设计，易于贡献
