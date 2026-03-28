# Agent loop runtime (内生引擎)

## Turn order (fixed; aligns with DeerFlow `before_turn` + LangChain agent middleware)

Per iteration of `run_agent_loop`, the **canonical order** is:

1. **`apply_lead_kernel_turn`** — shared `RuntimeKernel::prepare_with_input` (DeerFlow-style `before_turn` / lead pipeline). Merges loop tags, todos, memory snippets into `ThreadState`.
2. **`LoopStage::PreModel`** — skill injection + memory `retrieve` into `state.memory_working_set`.
3. **`AgentLoopMiddleware::before_model`** — runs after messages are built from `ThreadState` + skill preamble; may mutate `messages_for_llm` or `ThreadState`.
4. **`LoopStage::Model`** — `LLMPort::infer_turn`.
5. **`LoopStage::PostModel`** — `AgentLoopMiddleware::after_model` only (no checkpoint).
6. **Dispatch** — `classify_llm_routing` / `EngineCommand::from_llm_output` (see also `classify_turn_outcome`) → clarify / subagent / tools / text+memory branches, executed by `dispatch::execute_engine_command`, each with their own `LoopStage` and `commit_step` coordinated from `engine_v2.rs`.  
   - **Superstep scheduling (P1 kernel)**: each outer iteration calls `superstep_kernel::prepare_tasks` (clears `staged_tasks`), stages PULL tasks per phase via `superstep_kernel::prepare::prepare_pull_task` (lead / premodel / model / postmodel), and PUSH fan-out for tool batches / subagent plans (`prepare_tool_fanout`, `prepare_subagent_fanout`). Channel bumps + pending write records use `superstep_kernel::apply_writes_after_node` (wraps `pregel::bump_after_node`) after side effects; dispatch branches use `dispatch_*` node ids.  
   - **Lead template**: `LeadRuntimeSpec` on `AgentLoopRunConfig` gates lead kernel vs PreModel skill/memory without forking middleware types.

Hosts must not reorder **lead kernel** vs **before_model** without updating this document; doing so breaks parity with DeerFlow’s “lead before model call” semantics.

## Resume and checkpoints

- **`GraphRuntime::start_run`** writes an initial checkpoint; **`commit_step`** advances `step_seq` and persists `ThreadState` + metadata JSON (see `commit_metadata.rs` keys).
- **`GraphRuntime::resume_run`** loads the **latest** checkpoint for a `(thread_id, run_id)` and applies `CheckpointRecord::engine.pending_state_effects` to `ThreadState` when non-empty (crash-recovery replay). Callers that need a fresh run should allocate a new `run_id` via `start_run`; resuming continues from the last committed snapshot.
- **`ThreadState::state_schema_version`** (`agent-ports`) must be bumped when serde shape changes; older checkpoints may require an explicit migration path in the host (not implemented in this crate).

## Budget and errors

- **`RunBudget::max_turns`** — exceeding returns `AgentLoopError::MaxTurnsExceeded` (stable variant for mapping).
- Subagent plans are **truncated** to `min(max_subagent_tasks, subagent_task_cap_per_response)` with a warning log when needed; metadata records `subagent_plan_truncated`.
