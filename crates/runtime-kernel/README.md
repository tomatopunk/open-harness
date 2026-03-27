# runtime-kernel

Shared runtime kernel used by:
- `runtime-llm-chain-adapter`

## Scope

- `Thread`-level middleware context
- runtime events and tool invocation schema
- subagent executor with concurrency and timeout guardrails
- reusable kernel pipeline for adapter implementations

## Example

Run the example:

```bash
cargo run -p runtime-kernel --example minimal
```
