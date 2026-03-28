# 与 LangChain / LangGraph / Deer-Flow 的映射（内生引擎）

仅架构与执行语义；不含可观测性。

## LangGraph

| 概念 | harness 落点 |
|------|----------------|
| Pregel 超步 / 任务调度 | `engine_v2::run_agent_loop` 外层 turn + `pregel::bump_after_node` + `dispatch::execute_engine_command` |
| State + reducer | `agent_ports::StateEffect` + `StatePatch` 批量应用 |
| Checkpoint 元数据 | `CheckpointRecord::engine`（`EngineCheckpointExtensions`） |
| Command 路由 | `classify_llm_routing` / `EngineCommand::from_llm_output` |
| 并行 ToolNode | `invoke_tools_bounded` + `RunBudget::max_concurrent_tool_calls` |

## LangChain

| 概念 | harness 落点 |
|------|----------------|
| Runnable 组合 | 端口 trait（`LLMPort`, `ToolPort`）+ `AgentLoopMiddleware` 链 |
| 配置传播 | `AgentLoopRunConfig::configurable` + `LlmTurnContext` |
| 重试 / fallback | 仍由适配器或后续 `Runnable`-style 包装承担（本迭代未引入全局策略对象） |

## Deer-Flow

| 概念 | harness 落点 |
|------|----------------|
| 厚 middleware | `RuntimeKernel` / `apply_lead_kernel_turn` + `AgentLoopMiddleware` |
| Dangling tool 修复 | `repair_missing_tool_results` |
| 循环检测 / 去 tool_calls | `apply_repeated_tool_loop_breaker` |
| 子任务 per-response 限额 | `min(max_subagent_tasks, subagent_task_cap_per_response)` 截断 |
