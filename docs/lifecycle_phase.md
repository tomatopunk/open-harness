Open Harness Kernel Lifecycle Phases

- InitConfig: Load and validate configuration necessary for providers and components.
- InitProvider: Initialize LLM providers based on configuration (provider bootstrapping).
- InitMcp: Establish and configure MCP bridge/resources required for external capability access.
- InitPlugin: Discover and prepare plugins, but do not load or start yet.
- InitLoopState: Prepare the agent loop state (internal memory/cache initialization, readiness flags).
- Init: Core initialization steps, including memory, channel managers, and other core subsystems.
- AfterInit: Finalize initialization, perform any user-visible or system-wide post-init tasks.
- BeforeStart / Start / AfterStart: Standard lifecycle steps for runtime operations (unchanged).
- BeforeStop / Stop / AfterStop / Cleanup: Standard lifecycle steps for graceful shutdown (unchanged).

Rationale
- Breaking initialization into explicit stages provides finer-grained control, clearer error context, easier debugging, and safer incremental adoption of future features.
- `Init` and `AfterInit` remain in the runtime sequence because they are the stable hand-off between subsystem setup and the runnable kernel state machine.

Usage notes
- Tests should verify the new sequence order, and failure paths should propagate meaningful context for each stage.
- Providers and MCP initializations can fail gracefully with contextual error messages.
