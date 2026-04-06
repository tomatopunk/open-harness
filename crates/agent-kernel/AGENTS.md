# AGENT-KERNEL CRATE

## OVERVIEW

Minimal agent kernel, providing lifecycle management, event bus, hook system, plugin coordination, LLM provider integration, and MCP bridge integration.

## STRUCTURE

```
agent-kernel/
├── src/
│   ├── lib.rs          # Library entry point
│   ├── kernel.rs       # Core kernel implementation
│   ├── config.rs       # Configuration management
│   ├── state_machine.rs # State machine
│   ├── error.rs        # Error types
│   ├── lifecycle.rs    # Lifecycle management
│   └── streaming_runtime.rs # Streaming runtime
└── Cargo.toml
```

## WHERE TO LOOK

| Task | Location |
|------|----------|
| Kernel core logic | kernel.rs |
| State machine | state_machine.rs |
| Configuration | config.rs |
| Lifecycle | lifecycle.rs |
| Error handling | error.rs |

## CONVENTIONS

- Use `KernelState` and `KernelEvent` for state management
- All errors unified through `KernelError`
- Lifecycle stages defined via `LifecycleStage` enum
