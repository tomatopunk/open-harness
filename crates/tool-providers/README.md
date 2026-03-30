# Tool Providers for Harness

本 crate 提供了统一的工具提供者抽象层，支持多种工具来源的集成。

## 架构概述

```
┌─────────────────────────────────────────┐
│         Tool Provider Discovery         │
│         (工具发现引擎)                   │
├─────────────────────────────────────────┤
│  ┌──────────┐  ┌──────────┐  ┌───────┐ │
│  │   MCP    │  │  Skill   │  │ Local │ │
│  │ Provider │  │ Provider │  │Provider│ │
│  └──────────┘  └──────────┘  └───────┘ │
└─────────────────────────────────────────┘
```

## 功能特性

### 1. MCP 工具提供者

支持 Model Context Protocol (MCP) 服务器：

```rust
use tool_providers::McpToolProvider;
use mcp_client::McpServerConfig;

let config = McpServerConfig {
    name: "github".to_string(),
    enabled: true,
    r#type: "stdio".to_string(),
    command: Some("npx".to_string()),
    args: vec!["-y".to_string(), "@modelcontextprotocol/server-github".to_string()],
    env: std::collections::HashMap::new(),
    url: None,
    oauth: None,
    description: Some("GitHub operations".to_string()),
    headers: std::collections::HashMap::new(),
};

let provider = McpToolProvider::new(config).await?;
```

### 2. Skill 工具提供者

支持从文件系统加载 Skills：

```rust
use tool_providers::SkillToolProvider;

let provider = SkillToolProvider::new("/path/to/skills".to_string())?;
let skills = provider.get_enabled_skills();
```

### 3. 本地工具提供者

支持 Rust 实现的本地工具：

```rust
use tool_providers::LocalToolProvider;
use tool_runtime::ToolRegistry;

let registry = Arc::new(ToolRegistry::new());
registry.register(Box::new(MyCustomTool));

let provider = LocalToolProvider::new(registry, manifests);
```

### 4. 工具发现引擎

统一管理所有工具提供者：

```rust
use tool_providers::ToolProviderDiscovery;
use std::sync::Arc;

let discovery = Arc::new(ToolProviderDiscovery::new(Some(&governance_root)));

// 添加提供者
discovery.add_provider(Arc::new(mcp_provider)).await;
discovery.add_provider(Arc::new(skill_provider)).await;
discovery.add_provider(Arc::new(local_provider)).await;

// 获取所有工具
let all_manifests = discovery.all_manifests().await?;

// 查找工具
let provider = discovery.find_provider_for_tool("github_search").await;
```

## 配置示例

### MCP 配置 (`governance/mcp.yaml`)

```yaml
servers:
  - name: github
    enabled: true
    type: stdio
    command: npx
    args:
      - "-y"
      - "@modelcontextprotocol/server-github"
    env:
      GITHUB_TOKEN: "${GITHUB_TOKEN}"
    description: GitHub operations

  - name: filesystem
    enabled: true
    type: stdio
    command: npx
    args:
      - "-y"
      - "@modelcontextprotocol/server-filesystem"
      - "/workspace"
    description: File system operations
```

### Skill 配置 (`governance/skills.yaml`)

```yaml
skills:
  - name: research
    enabled: true
    preamble: "Prefer primary sources; summarize findings before long tool chains."
  
  - name: code-review
    enabled: false
```

## 工具装配

使用 `ToolAssemblyPolicy` 动态装配工具：

```rust
use agent_ports::ToolAssemblyPolicy;

let policy = ToolAssemblyPolicy {
    allowed_tags: vec!["builtin".into(), "mcp".into()].into_iter().collect(),
    denied_tools: vec!["dangerous_tool".into()].into_iter().collect(),
    max_tools: 10,
    allow_high_risk: false,
    ..Default::default()
};

let assembled_tools = policy.resolve(&all_manifests);
```

## 中间件集成

在 agent loop 中使用中间件链：

```rust
use agent_loop_runtime::middleware::MiddlewareChainBuilder;

let middleware_chain = MiddlewareChainBuilder::new()
    .with_tool_assembly(policy, all_manifests)
    .with_skill_injection(skill_port)
    .build();
```

## 运行示例

```bash
# 运行集成示例
cargo run --example tool-system-integration
```

## 扩展指南

### 创建新的工具提供者

实现 `ToolProvider` trait：

```rust
use agent_ports::{ToolProvider, ToolProviderType, ToolManifest, ToolCallSpec, ToolResult, HealthStatus};
use async_trait::async_trait;

pub struct MyCustomProvider {
    // 提供者状态
}

#[async_trait]
impl ToolProvider for MyCustomProvider {
    fn provider_type(&self) -> ToolProviderType {
        ToolProviderType::Community
    }

    fn provider_name(&self) -> &str {
        "my-custom"
    }

    async fn list_tools(&self) -> PortResult<Vec<ToolManifest>> {
        // 返回工具清单
    }

    async fn invoke(&self, call: &ToolCallSpec) -> PortResult<ToolResult> {
        // 调用工具
    }

    async fn health_check(&self) -> PortResult<HealthStatus> {
        // 健康检查
    }
}
```

## 设计模式

本实现参考了 DeerFlow 的架构，采用以下设计模式：

1. **配置驱动**: 所有组件通过 YAML/JSON 配置声明
2. **懒加载 + 缓存失效**: 工具首次使用时加载，检测配置变更自动失效
3. **中间件链**: 灵活组合横切关注点
4. **异步同步适配器**: 处理不同执行模型
5. **装饰器模式**: 添加横切逻辑（如审计日志）

## 许可证

MIT
