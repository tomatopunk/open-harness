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

Default startup reads `config.yaml` (auto-generated if missing). Environment variables with prefix `OPEN_HARNESS_` override YAML fields.

Useful config files:

- `config.example.yaml`: model/storage template
- `config.yaml`: active runtime config

Storage modes:

- `local_fs` (dev default)
- `sqlite`
- `postgres`
- `redis`
- `s3`

Env overrides (nested with `__`):

- `OPEN_HARNESS_GATEWAY__BIND` — default `0.0.0.0:8080`
- `OPEN_HARNESS_GATEWAY__LANGGRAPH_UPSTREAM` — LangGraph server base URL (default `http://127.0.0.1:2024`)
- `OPEN_HARNESS_MANAGE__BIND` — default `0.0.0.0:8081`
- `OPEN_HARNESS_MANAGE__LANGGRAPH_URL` — used for remote thread `DELETE`
- `OPEN_HARNESS_MANAGE__THREADS_ROOT` — local thread dirs (default `.deer-flow/threads`)
- `OPEN_HARNESS_CHANNEL__GATEWAY_URL` — channel service callback target (default `http://127.0.0.1:8080`)
- `OPEN_HARNESS_CONFIG_PATH` — override config yaml path

## API Contract

- Frozen contract doc: `docs/API_CONTRACT.md`
- OpenAI compatibility:
  - `GET /v1/models`
  - `POST /v1/chat/completions` (supports `stream=true|false`)
- DeerFlow-like manage APIs are exposed under `/api/*` from `open-harness-manage`.

## Smoke (curl)

With services running (see `scripts/smoke.sh`):

```bash
./scripts/smoke.sh
```

## Docker Compose

Set env vars:

```bash
cp .env.example .env
# 必填：设置真实 LangGraph 上游地址（例如 http://host.docker.internal:2024）
```

Fast local profile (recommended for iteration):

```bash
docker compose --profile dev-fast -f deploy/docker/docker-compose.yml up --build
```

Full stack profile (includes postgres/redis):

```bash
docker compose --profile full -f deploy/docker/docker-compose.yml up --build
```

This compose file runs backend stack for local iteration (without mock upstream):

- `open-harness-gateway`
- `open-harness-manage`
- `open-harness-channel`
- `open-harness-orchestrator`
- `redis`, `postgres`

Note: You must provide a real upstream via `OPEN_HARNESS_MODEL_URL`.

Thread workspace persists in Docker volume:

- `OPEN_HARNESS_MANAGE__THREADS_ROOT=/data/threads`

After startup, run:

```bash
./scripts/smoke.sh
```
