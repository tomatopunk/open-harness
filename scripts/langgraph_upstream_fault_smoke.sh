#!/usr/bin/env bash
# Manual / CI helper: verify gateway behavior when LangGraph upstream is unavailable.
# Start gateway with OPEN_HARNESS_GATEWAY__LANGGRAPH_UPSTREAM=http://127.0.0.1:9 (closed port),
# then expect non-success on proxied routes or OpenAI path per deployment.
set -euo pipefail
GATEWAY="${GATEWAY_URL:-http://127.0.0.1:8080}"
echo "Calling chat completions (upstream may return 502 if LangGraph unreachable)"
CODE="$(curl -sS -o /tmp/lg_fault_body.json -w "%{http_code}" -X POST "$GATEWAY/v1/chat/completions" \
  -H 'Content-Type: application/json' \
  -d '{"model":"gpt-4","messages":[{"role":"user","content":"ping"}],"user":"fault-smoke"}' || true)"
echo "HTTP $CODE"
head -c 400 /tmp/lg_fault_body.json 2>/dev/null || true
echo ""
echo "Done (document outcome in DEERFLOW_CAPABILITY_MATRIX E2E row)."
