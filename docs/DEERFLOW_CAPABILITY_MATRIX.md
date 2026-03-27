# DeerFlow Capability Matrix

This matrix tracks parity between `harness` and DeerFlow 2.0.

Status:
- `done`: implemented and wired in runtime path
- `partial`: interfaces exist but runtime behavior is incomplete
- `missing`: not implemented

| Domain | Capability | DeerFlow | Harness | Status | Evidence |
|---|---|---|---|---|---|
| Runtime | Lead runtime middleware chain | yes | partial | partial | `crates/orchestrator-core/src/middleware.rs` |
| Runtime | Thread state reducer contract | yes | partial | partial | `crates/orchestrator-core/src/pipeline.rs` |
| Runtime | Subagent execution pool/timeout | yes | partial | partial | `crates/orchestrator-core/src` |
| Runtime | LLM-Chain execution line | yes (langchain) | partial | partial | `crates/runtime-llm-chain-adapter` |
| Runtime | LLM-Chain adapter runtime | yes (langchain) | partial | partial | `crates/runtime-llm-chain-adapter` |
| Tooling | MCP transport/cache/auth | yes | partial | partial | `apps/manage/src/main.rs` |
| Tooling | Skills scan/install/enable/use | yes | partial | partial | `apps/manage/src/main.rs` |
| Sandbox | Provider abstraction | yes | partial | partial | `crates/sandbox-runtime/src/traits.rs` |
| Storage | Runtime backend switch | yes | partial | partial | `apps/manage/src/main.rs` |
| Channel | dingtalk driver | n/a | partial | partial | `crates/channel-dingtalk/src/lib.rs` |
| Channel | wecom driver | n/a | partial | partial | `crates/channel-wecom/src/lib.rs` |
| API | OpenAI compatible API | yes | yes | done | `apps/gateway/src/main.rs` |
| API | Manage `/api/*` contract | yes | yes | done | `docs/API_CONTRACT.md` |
| Obs | OTLP tracing | yes | partial | partial | `apps/gateway/src/main.rs` |
| Docs | OpenAPI docs | yes | partial | partial | `apps/gateway/src/main.rs` |

## LangChain / LLM-Chain Mapping

| DeerFlow LangChain Concept | Harness Mapping |
|---|---|
| model + tool call loop | `runtime-kernel` middleware pipeline + tool invocation records |
| runtime `configurable` | `protocol-compat::Configurable` |
| stream events | `runtime-kernel::RuntimeEvent` |
| task decomposition | `runtime-kernel::SubagentExecutor` |
| plan mode | `Configurable.is_plan_mode` + todo middleware |
