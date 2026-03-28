# 内生引擎基线（实现约束）

本文档描述当前 `agent-loop-runtime` 内生循环的**稳定契约**，供大步替换时对齐回归测试与宿主集成。

## 分层

| 层 | 职责 | 主要位置 |
|----|------|----------|
| 端口契约 | LLM / Tool / Memory / Skill / Subagent / Checkpoint | `agent-ports` |
| 图生命周期 | `start_run` / `commit_step` / `complete_run` | `graph-runtime-core` |
| 主循环 | `prepare_tasks` → … → `apply_writes_after_node`（外层 turn）+ **`build_dispatch_plan_with_options` → `execute_dispatch_plan`** | `engine_v2.rs`, `lead_outer_superstep.rs`, `superstep_kernel.rs`, `scheduler.rs`, `dispatch.rs`, `loop_common.rs` |
| 状态归约 | `StateEffect`（`agent-ports`）/ `StatePatch` | `state_effect.rs`（ports）, `turn_reducer.rs`, `state_patch.rs` |
| 健壮性 | 缺失 tool 结果修复、重复 tool 指纹熔断 | `loop_hardening.rs` |

## 关键约束

1. **Command IR**：`agent-ports::build_dispatch_plan_with_options`（`parse → classify → normalize → validate` + 可选 `ProviderStrategy` / `ToolStrategy`）产出 `DispatchPlan`；`AgentLoopRunConfig` 承载策略；运行时经 **`dispatch::route_llm_output`**（无静默降级）→ **`execute_dispatch_plan`**。优先级：澄清 > 子代理 > 工具 > 文本+记忆。
2. **Checkpoint**：`CheckpointRecord` 含 `engine: EngineCheckpointExtensions`（schema 版本钉扎、`pending_state_effects`、`superstep_seq`、`pending_writes_count` 来自 `ThreadState::pregel::pending_write_queue` 长度、resume 游标占位）。
3. **`ThreadState::migrate_to_latest_schema`**：每轮运行入口调用；`THREAD_STATE_SCHEMA_VERSION` **5** 起 `PregelMeta` 含 `staged_tasks` / `pending_write_queue` / `channel_versions` / `versions_seen` / `last_resume_at`（任务信封、写队列与恢复游标审计）。
4. **Memory**：`MemoryPort::retrieve` 失败通过 `PortError` 上浮，不再静默空结果。
5. **工具执行**：同轮多工具并发上限为 `max_concurrent_tool_calls`；**结果按 `tool_call_id` 回填后再与原始 `calls` 顺序对齐**（`superstep_kernel::execute::invoke_tool_calls_in_call_order`），`before_tool_call` / `after_tool_call` 仍按调用列表顺序执行以保证 `&mut ThreadState` 安全。
6. **子代理计划**：单轮任务数 `min(max_subagent_tasks, subagent_task_cap_per_response)`（默认可通过 `RunBudget` 配置 per-response 上限）；`per_subagent_task_timeout` 来自 `RunBudget`。

## 相关文档

- [ARCHITECTURE.md](./ARCHITECTURE.md) — 阶段与恢复语义  
- [CROSS_REF.md](./CROSS_REF.md) — 与 LangChain / LangGraph / Deer-Flow 的映射  
