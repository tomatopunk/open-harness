# open-harness

Rust implementation of the **open-harness** control/data plane aligned with [deer-flow](https://github.com/bytedance/deer-flow): LangGraph-compatible gateway, manage API, orchestrator, IM channels, and pluggable storage.

## Layout

- `apps/`: runtime services (`gateway`, `manage`, `channel`, `orchestrator`)
- `crates/`: reusable domain/runtime/storage crates
- `deploy/docker/`: Dockerfiles and compose for local multi-service run
- `scripts/`: smoke and developer helper scripts

## Build

```bash
cargo build --workspace --release
cargo test --workspace
cargo clippy --workspace -- -D warnings
```

## Configuration

Uses `figment` + env prefix `OPEN_HARNESS_` with `__` nesting:

- `OPEN_HARNESS_GATEWAY__BIND` — default `0.0.0.0:8080`
- `OPEN_HARNESS_GATEWAY__LANGGRAPH_UPSTREAM` — LangGraph server base URL (default `http://127.0.0.1:2024`)
- `OPEN_HARNESS_MANAGE__BIND` — default `0.0.0.0:8081`
- `OPEN_HARNESS_MANAGE__LANGGRAPH_URL` — used for remote thread `DELETE`
- `OPEN_HARNESS_MANAGE__THREADS_ROOT` — local thread dirs (default `.deer-flow/threads`)

## Smoke (curl)

With services running (see `scripts/smoke.sh`):

```bash
./scripts/smoke.sh
```

## Docker Compose

```bash
docker compose -f deploy/docker/docker-compose.yml up --build
```

Run **LangGraph** separately (e.g. upstream `langgraph dev`) and point `OPEN_HARNESS_GATEWAY__LANGGRAPH_UPSTREAM` at it.
