# AGENTS.md

## Cursor Cloud specific instructions

### Overview

open-harness is a Rust workspace (edition 2021) providing an AI gateway and control-plane.
Standard dev commands are defined in the `Makefile` and `.cargo/config.toml` aliases.

### Quick reference

| Action | Command |
|---|---|
| Build | `make build` or `cargo build --workspace --all-targets` |
| Lint | `make lint` or `cargo lint` |
| Format check | `make fmt-check` |
| Test | `make test` or `cargo xtest` |
| Pre-commit | `make pre-commit` (= fmt-check + lint + test) |
| Run gateway | `cargo run -p open-harness-gateway` (port 8080) |
| Run manage | `cargo run -p open-harness-manage` (port 8081) |
| Run channel | `cargo run -p open-harness-channel` (port 8082) |
| Run orchestrator | `cargo run -p open-harness-orchestrator` (port 8083) |
| Smoke test | `./scripts/smoke.sh` (requires all 4 services running) |

### Non-obvious caveats

- **Rust version**: The `Cargo.lock` pulls crate versions (e.g. `home`, `serde_with`) that require `rustc >= 1.88`. Use `rustup default stable` to ensure the latest stable toolchain. The workspace MSRV field says 1.75 but current locked deps need a newer compiler.
- **System dependency**: `libssl-dev` (or equivalent) must be installed for the `openssl-sys` crate to compile.
- **Storage mode**: Default is `local_fs` — no external databases needed. Directories are auto-created under `.deer-flow/local-fs/` on first run.
- **LangGraph upstream**: `POST /v1/chat/completions` proxies to a LangGraph server at `http://127.0.0.1:2024` by default. Without one, that endpoint returns `400 unknown model`. All other endpoints work without an upstream.
- **Parallel `cargo run`**: When starting multiple services simultaneously via background `cargo run`, they may block on Cargo's artifact directory lock. Start them sequentially or use pre-built binaries (`cargo build` first, then run `target/debug/open-harness-*` directly).
- **Smoke script deps**: `scripts/smoke.sh` requires `python3` and `curl` for UUID generation and JSON parsing.
