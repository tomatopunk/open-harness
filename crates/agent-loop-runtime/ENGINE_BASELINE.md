# 内生引擎基线（实现约束）

本文档描述当前 `agent-loop-runtime` 内生循环的**稳定契约**，供大步替换时对齐回归测试与宿主集成。

## 分层

| 层 | 职责 | 主要位置 |
|----|------|----------|
| 端口契约 | LLM / Tool / Memory / Skill / Subagent / Checkpoint | `agent-ports` |
| 图生命周期 | `start_run` / `commit_step` / `complete_run` | `graph-runtime-core` |
| 主循环 | Lead → PreModel → Model → PostModel → **EngineCommand** 执行 | `loop_engine.rs`, `execution_kernel.rs` |
| 状态归约 | `TurnEffect` / `StatePatch` | `turn_reducer.rs`, `state_patch.rs` |
| 健壮性 | 缺失 tool 结果修复、重复 tool 指纹熔断 | `loop_hardening.rs` |

## 关键约束

1. **`EngineCommand`**（`agent-ports::engine_command`）是模型输出后的唯一路由结果；优先级：澄清 > 子代理 > 工具 > 文本+记忆。
2. **Checkpoint**：`CheckpointRecord` 含 `engine: EngineCheckpointExtensions`（schema 版本钉扎、pending_writes 计数、resume 游标占位）。
3. **`ThreadState::migrate_to_latest_schema`**：每轮运行入口调用，用于 N−1 schema 演进。
4. **Memory**：`MemoryPort::retrieve` 失败通过 `PortError` 上浮，不再静默空结果。
5. **工具执行**：同轮多工具 **`buffer_unordered(max_concurrent_tool_calls)`**；`before_tool_call` / `after_tool_call` 仍顺序执行以保证 `&mut ThreadState` 安全。
6. **子代理计划**：单轮任务数 `min(max_subagent_tasks, 4)`（DeerFlow 式 per-response 上限）。

## 相关文档

- [ARCHITECTURE.md](./ARCHITECTURE.md) — 阶段与恢复语义  
- [CROSS_REF.md](./CROSS_REF.md) — 与 LangChain / LangGraph / Deer-Flow 的映射  
