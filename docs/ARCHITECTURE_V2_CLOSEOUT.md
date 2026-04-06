# Open Harness 架构 V2 收口说明

## 收口目标

本说明只覆盖本轮已完成的架构重构收口：同步当前实现的主链路依赖方向、边界摘要、验证入口与发布前使用方式。

## 主链路依赖图（当前实现）

```text
apps/kernel
  └── agent-kernel
        ├── plugin-system
        ├── llm-providers
        ├── mcp-bridge
        ├── state-abstraction
        │     ├── agent-ports
        │     └── unified-config
        └── unified-config

plugins/gateway-plugin ──┐
plugins/manage-plugin  ──┼──> plugin-system
plugins/dingtalk-plugin ─┘
plugins/dingtalk-plugin ─────> agent-kernel
```

## 边界摘要

### 1. Kernel 编排边界

- `apps/kernel` 仅保留 CLI / 进程入口。
- `agent-kernel` 是主链路编排中心，负责生命周期阶段、跨子系统初始化顺序、统一错误抬升与运行时配置接入。
- `agent-kernel` 依赖下游能力模块，但下游模块不反向拥有 kernel 生命周期。

### 2. Config 边界

- `unified-config` 是统一配置入口与优先级收敛点。
- `agent-kernel` 通过运行时视图消费配置，而不是在内核里重复定义独立配置合并规则。
- breaking-change 迁移层仍位于 kernel 运行时配置边界，详见 `docs/BREAKING_CHANGES_MIGRATION.md`。

### 3. Plugin / MCP / State 边界

- `plugin-system` 负责插件发现、装载、阶段状态与生命周期错误归因。
- `mcp-bridge` 负责 MCP server 生命周期与工具发现/缓存边界。
- `state-abstraction` 负责 registry、memory persistence 与状态后端抽象；其上游契约经 `agent-ports` 暴露稳定执行接口。

### 4. 补充 workspace 成员

- `ecosystem-registry`、`package-manager` 当前仍在 workspace 中，但不属于本轮 1-12 主链路边界重构的收口核心。
- `dingtalk-plugin` 当前同时依赖 `plugin-system` 与 `agent-kernel`，这是 closeout 阶段需显式记录的补充依赖，而不是新的重构范围。

## 文档与实现对齐结论

- `docs/ARCHITECTURE_V2.md` 仍然保留目标态说明，但已补充“当前实现状态（收口同步）”入口，避免把目标结构误读为全部已完成状态。
- CI 的默认验收面已与 Task 11 对齐，显式包含 e2e acceptance matrix。
- 本轮 closeout 不扩展到计划中尚未开始的 Session Core / State Machine / Streaming Tool Runtime / Security Chain / Memory roadmap 任务。

## 发布前使用方式

1. 先看 `docs/RELEASE_READINESS_CHECKLIST.md`，按 checklist 逐项确认。
2. 再看 `.sisyphus/evidence/task-12-closeout.md`，确认证据路径、命令和结论完整。
3. 最后执行 workspace 级验证：`cargo check --workspace`、`cargo test --workspace`。
