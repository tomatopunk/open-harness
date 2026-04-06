# E2E TESTS

## OVERVIEW

End-to-end test suite for verifying complete Open Harness functionality.

## STRUCTURE

```
e2e/
├── src/
│   ├── lib.rs          # Library entry point
│   ├── tests/          # Test files
│   │   ├── gateway.rs  # Gateway tests
│   │   ├── mcp.rs      # MCP tests
│   │   ├── agent_loop.rs # Agent loop tests
│   │   └── common.rs   # Common tests
│   ├── helpers/        # Test helpers
│   └── fixtures/       # Test fixtures
├── Cargo.toml
└── README.md
```

## WHERE TO LOOK

| Task | Location |
|------|----------|
| Gateway tests | tests/gateway.rs |
| MCP tests | tests/mcp.rs |
| Agent loop tests | tests/agent_loop.rs |
| Test helpers | helpers/ |
| Test fixtures | fixtures/ |

## COMMANDS

```bash
# Run all E2E tests
make e2e

# Or directly with Cargo
cargo test --manifest-path e2e/Cargo.toml --tests
```
