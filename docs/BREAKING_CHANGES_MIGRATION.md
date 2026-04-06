# Breaking Changes 迁移层与回滚说明

## 迁移点

- `apps/kernel/src/main.rs` 不再直接把 `config.yaml` 视为唯一运行时入口，而是先经过 `KernelConfig::resolve_runtime(...)`。
- `crates/agent-kernel/src/config.rs` 新增三种运行模式：`legacy`、`unified`、`auto`。
- `crates/agent-kernel/src/config.rs` 在 `unified/auto` 模式下，会把统一配置视图中的 LLM 选择结果适配回现有 `KernelConfig.llm`，避免 kernel、plugin、state 边界被硬切换。
- `crates/agent-kernel/src/config.rs` 在 `unified/auto` 模式下，会把 `extensions_config.json` 中的 MCP servers 适配为现有 `mcp_bridge::McpBridgeConfig`。
- `crates/agent-kernel/src/kernel.rs` 改为使用解析后的 `config.mcp` 初始化 MCP bridge，保留现有 kernel 生命周期与 CLI 行为。

## 模式说明

- `legacy`：只使用现有 `config.yaml -> KernelConfig` 路径，作为稳定回滚模式。
- `unified`：强制启用统一配置迁移层；统一配置解析失败时直接报错，不自动回退。
- `auto`：优先启用统一配置迁移层；若统一配置解析失败且 `rollback_on_unified_failure=true`，自动回退到 `legacy`。

运行模式优先级：`OPEN_HARNESS_KERNEL_MODE` 环境变量 > `config.yaml:migration.mode`。

## 配置示例

```yaml
migration:
  mode: auto
  governance_root: governance
  rollback_on_unified_failure: true
```

## 迁移步骤

1. 保持现有 `config.yaml` 可启动，先把 `migration.mode` 设为 `auto`。
2. 确认 `governance/models.yaml` 的 `default_model` 能命中 legacy `llm.model` 映射出的模型名。
3. 确认 `extensions_config.json` 中的 MCP server 条目已完整声明 `enabled/type/command|url`。
4. 启动 kernel，观察日志中的 `requested` / `effective` mode。
5. 当 `effective=unified` 且核心路径验证通过后，再把模式切到 `unified`。

## 回滚点

- 回滚点 1：`governance/models.yaml` 的 `default_model` 与当前 legacy LLM 映射名不一致。
- 回滚点 2：`extensions_config.json` 解析失败或 MCP transport 不合法。
- 回滚点 3：统一配置映射到 provider 类型时无法识别。

以上任一点在 `auto` 模式下都会回落到 `legacy`，并在日志中记录回滚原因。

## 回滚步骤

1. 立即把 `OPEN_HARNESS_KERNEL_MODE=legacy`，或把 `config.yaml:migration.mode` 改回 `legacy`。
2. 重启 kernel，确认日志中 `effective=legacy`。
3. 保留原 `config.yaml` 的 `llm/storage/channels/plugins_dir/workspace_root`，不要同时修改统一配置与旧配置。
4. 修复统一配置问题后，再重新使用 `auto` 做一次预演。

## 恢复顺序

1. 先恢复 kernel 启动模式为 `legacy`。
2. 再检查 `governance/models.yaml` 与 `extensions_config.json`。
3. 再验证 MCP server 适配结果。
4. 最后重新启用 `auto`，确认 `effective=unified` 后再切 `unified`。

## 已覆盖的核心验证路径

- `unified` 模式：legacy `llm` 通过统一配置视图完成 provider 选择，并把 `extensions_config.json` 中的 MCP server 适配进 `config.mcp`。
- 回滚兼容模式：`auto` 模式在统一配置失配时回落到 `legacy`，保留原有 `llm` 运行参数。
