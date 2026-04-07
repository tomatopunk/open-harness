# OPEN HARNESS KNOWLEDGE BASE

**Generated:** 2026-04-06
**Commit:** 3826c14
**Branch:** main

## OVERVIEW

Open Harness - A truly open agent kernel architecture, inspired by oh-my-openagent's plugin design and rig library's mature ecosystem. Rust project using Cargo workspace.

## STRUCTURE

```
open-harness/
├── apps/                    # Binary applications
│   └── kernel/             # Core kernel binary
├── crates/                  # Core libraries
│   ├── agent-kernel/       # Minimal agent kernel
│   ├── plugin-system/      # Plugin system
│   ├── llm-providers/      # Generic LLM provider abstraction
│   ├── mcp-bridge/         # Enhanced MCP bridge
│   ├── unified-config/     # Unified configuration
│   ├── agent-ports/        # Port abstractions (20 files)
│   ├── state-abstraction/  # State abstraction (19 files)
│   ├── ecosystem-registry/ # Ecosystem registry
│   └── package-manager/    # Package manager
├── plugins/                 # Plugin directory
│   ├── gateway-plugin/     # API gateway plugin
│   ├── manage-plugin/      # Management plugin
│   └── dingtalk-plugin/    # DingTalk plugin
├── e2e/                     # End-to-end tests
├── docs/                    # Documentation
├── skills/                  # Skills directory (MCP server format)
├── governance/              # Governance configuration
├── migrations/              # Database migrations
├── scripts/                 # Script tools
└── deploy/                  # Deployment configuration
```

## WHERE TO LOOK

| Task | Location | Notes |
|------|----------|-------|
| Kernel implementation | crates/agent-kernel/ | Core logic |
| Plugin development | crates/plugin-system/ | Plugin trait |
| LLM integration | crates/llm-providers/ | Provider abstraction |
| MCP integration | crates/mcp-bridge/ | MCP bridge |
| Configuration management | crates/unified-config/ | Unified config |
| Port abstractions | crates/agent-ports/ | 20 files |
| State management | crates/state-abstraction/ | 19 files |
| E2E tests | e2e/ | End-to-end tests |
| Plugin examples | plugins/ | gateway/manage/dingtalk |

## CONVENTIONS

- **Rust Edition**: 2021
- **Rust Version**: 1.75+
- **Formatting**: `cargo fmt --all`
- **Linting**: `cargo clippy --workspace --all-targets -- -D warnings`
- **Testing**: `cargo test --workspace`
- **E2E**: `make e2e` or `cargo test --manifest-path e2e/Cargo.toml --tests`
- **Workspace Resolver**: 2
- **Unsafe Code**: Forbidden (`forbid(unsafe_code)`)

## ANTI-PATTERNS (THIS PROJECT)

- ❌ `unwrap()` - Forbidden (`deny(clippy::unwrap_used)`)
- ❌ `dbg!()` - Forbidden (`deny(clippy::dbg_macro)`)
- ❌ `todo!()` - Forbidden (`deny(clippy::todo)`)
- ❌ `unsafe` - Forbidden (`forbid(unsafe_code)`)
- ❌ Ignoring `#[must_use]` - Forbidden (`deny(unused_must_use)`)

## UNIQUE STYLES

- **Cargo Workspace**: Multi-crate management
- **Makefile**: Common command wrappers
- **Plugin Design**: Everything is a plugin
- **MCP First**: External capabilities via MCP
- **Configuration Driven**: Layered configuration system
- **Hook System**: Lifecycle hooks support extensions

## COMMANDS

```bash
# Development mode
make dev

# Code formatting
make fmt

# Full check (formatting + lint + tests + e2e)
make check

# Pre-commit check
make pre-commit

# Build
make build

# Test
make test

# Lint
make lint

# E2E tests
make e2e

# Clean
make clean
```

## NOTES

- **Deleted**: `runtime-langgraph-adapter`, `mcp-client` crates (deprecated)
- **Deleted**: `src/` directory (TypeScript legacy files)
- **Deleted**: Outdated V2 documentation
- **Configuration files**: `config.yaml`, `extensions_config.json`
- **Local development**: Uses `.data/local-fs/` storage
