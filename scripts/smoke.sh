#!/usr/bin/env bash
set -euo pipefail
GATEWAY="${GATEWAY_URL:-http://127.0.0.1:8080}"
MANAGE="${MANAGE_URL:-http://127.0.0.1:8081}"
CHANNEL="${CHANNEL_URL:-http://127.0.0.1:8082}"
ORCH="${ORCH_URL:-http://127.0.0.1:8083}"

echo "== health checks"
curl -fsS "$GATEWAY/healthz"
curl -fsS "$MANAGE/healthz"
curl -fsS "$CHANNEL/healthz"
curl -fsS "$ORCH/healthz"

echo ""
echo "== gateway demo SSE"
curl -fsSN "$GATEWAY/api/langgraph/demo-stream" | head -n 3

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
echo "== channel hooks"
curl -fsS -X POST "$CHANNEL/hooks/dingtalk" -H 'Content-Type: application/json' -d '{"text":"hello","eventId":"e1"}'
echo ""
curl -fsS -X POST "$CHANNEL/hooks/wecom" -H 'Content-Type: application/json' -d '{"Content":"hi","FromUserName":"u1"}'
echo ""

echo "== orchestrator pipeline"
curl -fsS -X POST "$ORCH/internal/pipeline-check"
echo ""
echo "OK"
