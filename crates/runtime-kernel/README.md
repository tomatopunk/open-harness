# runtime-kernel

Shared runtime kernel used by:
- `runtime-llm-chain-adapter`

## Scope

- `Thread`-level middleware context
- middleware chain with loop detection, guardrails, and tool error handling
- runtime events and tool invocation schema
- subagent executor with concurrency, timeout, and cancellation
- reusable kernel pipeline for adapter implementations

## Example

Run the example:

```bash
cargo run -p runtime-kernel --example minimal
```
