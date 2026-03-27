#!/usr/bin/env bash
set -euo pipefail
GATEWAY="${GATEWAY_URL:-http://127.0.0.1:8080}"
MANAGE="${MANAGE_URL:-http://127.0.0.1:8081}"
CHANNEL="${CHANNEL_URL:-http://127.0.0.1:8082}"
ORCH="${ORCH_URL:-http://127.0.0.1:8083}"
LOCAL_FS_ROOT="${LOCAL_FS_ROOT:-.deer-flow/local-fs}"

echo "== health checks"
curl -fsS "$GATEWAY/healthz"
curl -fsS "$MANAGE/healthz"
curl -fsS "$CHANNEL/healthz"
curl -fsS "$ORCH/healthz"

echo ""
echo "== gateway demo SSE"
curl -fsSN "$GATEWAY/api/langgraph/demo-stream" | head -n 3

echo ""
echo "== gateway openapi (schemas)"
curl -fsS "$GATEWAY/openapi.json" | grep -q ChatCompletionRequest

echo ""
echo "== openai models"
curl -fsS "$GATEWAY/v1/models"

echo ""
echo "== openai chat completion"
curl -fsS -X POST "$GATEWAY/v1/chat/completions" \
  -H 'Content-Type: application/json' \
  -d '{"model":"open-harness-default","messages":[{"role":"user","content":"hello"}],"user":"smoke-user"}'

echo ""
echo "== manage thread delete (async)"
TID="$(python3 -c 'import uuid; print(uuid.uuid4())' 2>/dev/null || uuidgen | tr '[:upper:]' '[:lower:]')"
RESP="$(curl -fsS -X DELETE "$MANAGE/api/manage/threads/$TID")"
echo "$RESP"
OP="$(echo "$RESP" | python3 -c 'import sys,json; print(json.load(sys.stdin)["operation_id"])' 2>/dev/null || echo "")"
if [[ -n "$OP" ]]; then
  curl -fsS "$MANAGE/api/manage/thread-delete-ops/$OP" | head -c 500
  echo ""
fi

echo "== storage switch stub"
curl -fsS -X POST "$MANAGE/api/manage/admin/storage/switch" \
  -H 'Content-Type: application/json' \
  -d '{"backend":"sqlite"}'
echo ""
echo "== storage runtime status"
curl -fsS "$MANAGE/api/manage/admin/storage/status"

echo ""
echo "== channel hooks"
curl -fsS -X POST "$CHANNEL/hooks/dingtalk" -H 'Content-Type: application/json' -d '{"text":"hello","eventId":"e1"}'
echo ""
curl -fsS -X POST "$CHANNEL/hooks/wecom" -H 'Content-Type: application/json' -d '{"Content":"hi","FromUserName":"u1"}'
echo ""

echo "== orchestrator pipeline (runtime_metadata)"
curl -fsS -X POST "$ORCH/internal/pipeline-check" | grep -q runtime_metadata
echo ""
echo "== manage core APIs"
curl -fsS "$MANAGE/api/models"
echo ""
curl -fsS "$MANAGE/api/skills"
echo ""
echo "== mcp oauth status"
curl -fsS "$MANAGE/api/mcp/oauth/status"
echo ""
echo "== install skill archive"
curl -fsS -X POST "$MANAGE/api/skills/install" -H 'Content-Type: application/json' \
  -d '{"archive_name":"demo.skill","enabled":true}'
echo ""
curl -fsS "$MANAGE/api/channels/"
echo ""
echo "== local_fs layout check"
for p in config tasks threads uploads artifacts memory; do
  test -d "$LOCAL_FS_ROOT/$p" || { echo "missing $LOCAL_FS_ROOT/$p"; exit 1; }
done
echo "local_fs ok"
echo ""
echo "== orchestrator orchestrate"
curl -fsS -X POST "$ORCH/internal/orchestrate" -H 'Content-Type: application/json' \
  -d '{"configurable":{"is_plan_mode":true,"subagent_enabled":true,"max_concurrent_subagents":2},"messages":["fact:smoke","hello"]}'
echo ""
if [[ -n "${OPEN_HARNESS_OTLP_ENDPOINT:-}" ]]; then
  echo "== OTLP: OPEN_HARNESS_OTLP_ENDPOINT is set; gateway should initialize tracing exporter when reachable"
fi

echo "OK"
