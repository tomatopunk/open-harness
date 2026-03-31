# Open Harness

> 一个真正开放的 agent 内核架构，参考 oh-my-openagent 的插件化设计和 rig 库的成熟生态系统。

## 核心特性

- **最小化内核** - 只保留 agent 循环、事件总线、插件管理
- **一切皆插件** - 模型、工具、渠道、存储都作为插件加载
- **MCP 优先** - 所有外部能力通过 MCP 提供
- **通用 LLM 抽象** - 不绑定任何特定库，支持多种 provider（rig、OpenAI、Anthropic 等）
- **插件化设计** - 参考 oh-my-openagent 的工厂模式和钩子系统
- **Claude 理念** - 最小化核心，最大化扩展能力

## 目录结构

```
open-harness/
├── apps/
│   └── kernel/              # 核心 kernel 二进制
├── crates/
│   ├── agent-kernel/        # 最小化 agent 内核
│   ├── plugin-system/       # 插件系统
│   ├── llm-providers/       # 通用 LLM provider 抽象
│   ├── mcp-bridge/          # 增强的 MCP 桥接
│   ├── agent-ports/         # 端口抽象（保留用于兼容性）
│   ├── state-abstraction/   # 状态抽象（保留用于兼容性）
│   └── mcp-client/          # MCP 客户端（保留用于兼容性）
└── plugins/                  # 插件目录
    ├── gateway-plugin/       # API 网关插件（OpenAI 兼容 API）
    └── manage-plugin/        # 管理插件
```

## 快速开始

```bash
# 构建
cargo build --workspace

# 运行 kernel
cargo run -p open-harness-kernel

# 运行测试
cargo test --workspace

# 运行 lint
cargo clippy --workspace -- -D warnings
```

## 核心设计原则

1. **最小化内核** - 只保留 agent 循环、事件总线、插件管理
2. **一切皆插件** - 模型、工具、渠道、存储都作为插件加载
3. **MCP 优先** - 所有外部能力通过 MCP 提供
4. **配置驱动** - 参考 oh-my-openagent 的分层配置系统
5. **钩子系统** - 生命周期钩子支持扩展
6. **LLM 抽象** - 通用的 LLM provider 层，rig 只是其中一个实现

## 插件开发

### 创建一个插件

1. 在 `plugins/` 目录创建插件目录
2. 创建 `plugin.yaml` 清单
3. 实现 `Plugin` trait

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

## 关于"Open"

Open Harness 真正做到了"Open"：

1. **开放架构** - 最小化内核，一切可扩展
2. **开放生态** - 支持多种 LLM provider（不只是 rig）
3. **开放协议** - MCP 优先，标准协议
4. **开放贡献** - 插件化设计，易于贡献

## License

MIT
