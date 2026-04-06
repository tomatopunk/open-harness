# Open Harness 测试计划

## 概述

本文档定义了 Open Harness 项目的完整测试策略，包括单元测试、集成测试和端到端测试。

## 测试现状分析

### 现有测试覆盖

| Crate | 测试数量 | 覆盖状态 |
|-------|---------|---------|
| agent-kernel | 28 | ✅ 良好 |
| agent-ports | 46 | ✅ 良好 |
| state-abstraction | 44 | ✅ 良好 |
| unified-config | 23 | ✅ 良好 |
| plugin-system | 5 | ⚠️ 不足 |
| mcp-bridge | 3 | ⚠️ 不足 |
| llm-providers | 0 | ❌ 缺失 |
| ecosystem-registry | 0 | ❌ 缺失 |
| package-manager | 0 | ❌ 缺失 |
| gateway-plugin | 0 | ❌ 缺失 |
| manage-plugin | 0 | ❌ 缺失 |
| dingtalk-plugin | 0 | ❌ 缺失 |

### E2E 测试现状

当前 e2e 测试采用 acceptance wrapper 风格：通过 `e2e/src/tests/*.rs` 将跨 crate 的关键回归场景绑定到独立 CI 门禁。

当前核心 acceptance 覆盖包括：
- `gateway.rs` - 生命周期阶段顺序与插件失败上下文
- `mcp.rs` - MCP 重连与工具缓存失效回归
- `agent_loop.rs` - 配置优先级与缺失内存后端回归
- `engine_certification.rs` - session → FSM → runtime → security → memory 闭环认证

**当前重点**：继续保持 wrapper 风格，但确保每个发布关键链路至少有一条完整认证场景，而不是只看单模块测试。

## 测试策略

### 测试金字塔

```
        /\
       /E2E\      <-- 端到端测试 (关键用户流程)
      /------\
     /集成测试\    <-- 模块间集成测试
    /----------\
   /  单元测试  \   <-- 函数/模块级测试 (基础)
  /--------------\
```

### 优先级定义

- **P0** - 阻塞发布，核心功能
- **P1** - 重要功能，应尽快完成
- **P2** - 增强功能，可以延后

## 单元测试计划

### P0 - 核心模块

#### 1. llm-providers
需要添加的测试：
- [ ] OpenAI provider 集成测试
- [ ] Rig provider 集成测试
- [ ] Provider 配置解析测试
- [ ] Message 序列化/反序列化测试
- [ ] 错误处理测试

#### 2. mcp-bridge
需要添加的测试：
- [ ] MCP server 连接管理测试
- [ ] 工具调用测试
- [ ] 重连机制测试 (已有)
- [ ] 缓存失效测试
- [ ] Skill MCP 集成测试

#### 3. plugin-system
需要添加的测试：
- [ ] 插件加载/卸载测试
- [ ] 插件生命周期钩子测试
- [ ] 插件依赖管理测试
- [ ] 插件清单解析测试

### P1 - 重要模块

#### 1. ecosystem-registry
需要添加的测试：
- [ ] 注册表搜索测试
- [ ] 组件元数据解析测试
- [ ] 缓存机制测试

#### 2. package-manager
需要添加的测试：
- [ ] NPM 包安装测试
- [ ] Cargo 包安装测试
- [ ] Pip 包安装测试
- [ ] 沙箱环境测试

#### 3. 插件
需要添加的测试：
- [ ] gateway-plugin - API 端点测试
- [ ] manage-plugin - 管理 API 测试
- [ ] dingtalk-plugin - 消息发送测试

## 集成测试计划

### P0 - 核心集成

#### 1. Kernel + Plugin System
- [ ] 插件加载到内核的集成测试
- [ ] 插件生命周期与内核状态机集成
- [ ] 插件错误处理与内核容错

#### 2. Kernel + MCP Bridge
- [ ] MCP 工具在内核中的可用性测试
- [ ] 工具调用与 agent loop 集成
- [ ] MCP 重连与内核恢复

#### 3. Kernel + LLM Providers
- [ ] LLM 调用与 agent loop 集成
- [ ] 流式响应处理测试
- [ ] 错误重试机制测试

## E2E 测试计划

### P0 - 核心用户流程

#### 1. 完整 Agent 执行流程
测试场景：
- 启动内核
- 加载配置
- 初始化插件
- 接收用户输入
- 执行 agent loop
- 返回结果

测试文件：`e2e/src/tests/full_agent_flow.rs`

#### 2. MCP 工具使用流程
测试场景：
- 启动内核
- 连接 MCP 服务器
- 列出可用工具
- 调用工具
- 验证结果

测试文件：`e2e/src/tests/mcp_integration.rs`

#### 3. 配置加载与切换
测试场景：
- 加载 legacy 配置
- 加载 unified 配置
- 配置热重载
- 验证配置生效

测试文件：`e2e/src/tests/config_loading.rs`

#### 4. Plugin 系统集成
测试场景：
- 加载多个插件
- 验证插件初始化
- 测试插件间通信
- 插件错误恢复

测试文件：`e2e/src/tests/plugin_integration.rs`

### P1 - 扩展流程

#### 1. Gateway API 完整流程
- [ ] 创建任务
- [ ] 查询任务状态
- [ ] 流式响应
- [ ] 任务取消

#### 2. 多渠道消息处理
- [ ] HTTP 渠道
- [ ] DingTalk 渠道
- [ ] 消息路由

#### 3. 状态持久化
- [ ] 会话创建
- [ ] 状态保存
- [ ] 状态恢复
- [ ] 并发安全

## 测试基础设施

### 现有设施

- ✅ Cargo test 集成
- ✅ E2E 测试框架基础
- ✅ 测试辅助函数 (`e2e/src/helpers/`)
- ✅ Makefile 命令包装

### 需要增强

1. **测试 Fixtures**
   - 测试配置文件
   - 模拟 MCP 服务器
   - 测试数据生成器

2. **测试 Utilities**
   - 内核测试运行器
   - API 客户端封装
   - 断言辅助函数

3. **测试环境**
   - 临时目录管理
   - 环境隔离
   - 清理机制

## 执行计划

### 阶段 1：E2E 测试增强 (优先)
1. 增强测试基础设施
2. 添加完整 agent 流程测试
3. 添加 MCP 集成测试
4. 添加配置加载测试

### 阶段 2：单元测试补充
1. llm-providers 测试
2. mcp-bridge 测试增强
3. plugin-system 测试增强
4. package-manager 测试

### 阶段 3：集成测试
1. 内核与各模块集成
2. 插件系统集成
3. MCP 桥接集成

### 阶段 4：文档与优化
1. 测试文档
2. CI/CD 集成
3. 测试覆盖率报告

## 测试命令

```bash
# 运行所有单元测试
make test

# 运行所有 E2E 测试
make e2e

# 运行完整检查
make check

# 运行特定 crate 测试
cargo test -p agent-kernel

# 运行特定 E2E 测试
cargo test --manifest-path e2e/Cargo.toml gateway
```

## 成功指标

- [ ] 所有 P0 测试完成
- [ ] 核心 crate 测试覆盖率 > 80%
- [ ] E2E 测试覆盖主要用户流程
- [ ] 所有测试在 CI 中通过
- [ ] 测试运行时间 < 5 分钟
