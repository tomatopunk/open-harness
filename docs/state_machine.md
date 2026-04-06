# Open Harness Kernel State Machine

## Overview

This document defines the formal Kernel State Machine model for the Open Harness kernel lifecycle. It specifies the allowed states, events, transitions, guards, and side effects.

## States

- `Created` - Initial state when the kernel is first instantiated
- `Initialized` - After successful initialization of all core components
- `Running` - Kernel is fully operational and processing events
- `Stopped` - Kernel has been gracefully stopped

## Events

- `Initialize` - Trigger initialization of core components
- `Start` - Start the kernel and all plugins/channels
- `Stop` - Stop the kernel and gracefully shut down
- `ToolExecutionStarted` - A tool execution has started
- `ToolExecutionFinished` - A tool execution has finished

## Transitions

| From State | Event | To State | Side Effects |
|------------|-------|----------|--------------|
| `Created` | `Initialize` | `Initialized` | InitializeProviderAdapter, InitializeMcpAdapter, InitializePluginAdapter, InitializeLoopStateAdapter, InitializeChannelAdapter, RunLifecycleStages(init stages) |
| `Initialized` | `Start` | `Running` | RunLifecycleStages(start stages), LoadPlugins, InitializePlugins, StartPlugins, StartChannels, PublishKernelStarted |
| `Running` | `Stop` | `Stopped` | PublishKernelStopped, StopChannels, StopPlugins, UnloadPlugins, RunLifecycleStages(shutdown stages) |
| `Running` | `ToolExecutionStarted` | `Running` | RecordToolExecutionStart |
| `Running` | `ToolExecutionFinished` | `Running` | RecordToolExecutionFinish |

## Guards

- `StateIs(KernelState)` - Ensures the kernel is in the expected state before allowing a transition

## Side Effects

### Initialization Side Effects
- `InitializeProviderAdapter` - Set up LLM provider adapters
- `InitializeMcpAdapter` - Set up MCP bridge
- `InitializePluginAdapter` - Set up plugin system
- `InitializeLoopStateAdapter` - Set up agent loop state
- `InitializeChannelAdapter` - Set up communication channels

### Runtime Side Effects
- `LoadPlugins` - Load all configured plugins
- `InitializePlugins` - Initialize loaded plugins
- `StartPlugins` - Start initialized plugins
- `StartChannels` - Start communication channels
- `PublishKernelStarted` - Announce kernel has started
- `PublishKernelStopped` - Announce kernel has stopped
- `StopChannels` - Stop communication channels
- `StopPlugins` - Stop running plugins
- `UnloadPlugins` - Unload stopped plugins

### Lifecycle Side Effects
- `RunLifecycleStages(Vec<LifecycleStage>)` - Execute sequence of lifecycle stages

### Tool Execution Side Effects
- `RecordToolExecutionStart { session_id, tool_call_id, tool_name }` - Record tool start
- `RecordToolExecutionFinish { session_id, tool_call_id, tool_name, success }` - Record tool finish

## Diagram (ASCII)

```
Created
   | Initialize
   v
Initialized
   | Start
   v
Running <--+
   |       | ToolExecutionStarted/ToolExecutionFinished
   | Stop  |
   v       +
Stopped
```

## Verification

- Unit tests cover all valid transition paths
- Negative tests verify invalid transitions are rejected with typed errors
- Side effects are verified to match expected orchestration behavior

## Usage Example (Rust)

```rust
use agent_kernel::state_machine::{KernelState, KernelEvent, KernelStateMachine};

let mut state = KernelState::Created;

// Initialize
let transition = KernelStateMachine::transition(state, KernelEvent::Initialize)?;
state = transition.to;
assert_eq!(state, KernelState::Initialized);

// Start
let transition = KernelStateMachine::transition(state, KernelEvent::Start)?;
state = transition.to;
assert_eq!(state, KernelState::Running);

// Stop
let transition = KernelStateMachine::transition(state, KernelEvent::Stop)?;
state = transition.to;
assert_eq!(state, KernelState::Stopped);
```
