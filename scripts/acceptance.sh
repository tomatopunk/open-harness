#!/usr/bin/env bash
set -euo pipefail

GATEWAY="${GATEWAY_URL:-http://127.0.0.1:8080}"
MANAGE="${MANAGE_URL:-http://127.0.0.1:8081}"
MODEL="${MODEL_NAME:-gpt-4}"
THREAD_ID="${THREAD_ID:-$(python3 -c 'import uuid; print(uuid.uuid4())')}"
AUTH_HEADER="${AUTH_HEADER:-}"
TENANT_ID="${TENANT_ID:-tenant-demo}"
USER_ID="${USER_ID:-user-demo}"

common_headers=(
  -H "x-tenant-id: ${TENANT_ID}"
  -H "x-user-id: ${USER_ID}"
)
if [[ -n "${AUTH_HEADER}" ]]; then
  common_headers+=(-H "${AUTH_HEADER}")
fi

echo "== gateway: non-stream completion (opencode/cursor compatible)"
curl -fsS -X POST "${GATEWAY}/v1/chat/completions" \
  "${common_headers[@]}" \
  -H 'Content-Type: application/json' \
  -d "{
    \"model\":\"${MODEL}\",
    \"user\":\"${USER_ID}\",
    \"messages\":[
      {\"role\":\"system\",\"content\":\"你是一个简洁助手\"},
      {\"role\":\"user\",\"content\":\"请回复ok\"}
    ],
    \"configurable\":{\"thinking_enabled\":true,\"subagent_enabled\":true}
  }" >/tmp/open-harness-gateway-nonstream.json
python3 - <<'PY'
import json
v=json.load(open('/tmp/open-harness-gateway-nonstream.json'))
assert v["object"]=="chat.completion"
assert v["choices"][0]["message"]["role"]=="assistant"
print("gateway non-stream ok")
PY

echo "== gateway: stream completion"
curl -fsSN -X POST "${GATEWAY}/v1/chat/completions" \
  "${common_headers[@]}" \
  -H 'Content-Type: application/json' \
  -d "{
    \"model\":\"${MODEL}\",
    \"stream\":true,
    \"user\":\"${USER_ID}\",
    \"messages\":[{\"role\":\"user\",\"content\":\"流式返回一个词\"}],
    \"stream_mode\":[\"values\",\"messages-tuple\",\"end\",\"error\"]
  }" | head -n 8

echo "== manage: create task (REST)"
TASK_RESP="$(curl -fsS -X POST "${MANAGE}/api/manage/threads/${THREAD_ID}/tasks" \
  "${common_headers[@]}" \
  -H 'Content-Type: application/json' \
  -d '{
    "stream": false,
    "input": {"messages":[{"role":"user","content":"任务测试"}]},
    "configurable": {"thinking_enabled": false}
  }')"
echo "${TASK_RESP}"
TASK_ID="$(echo "${TASK_RESP}" | python3 -c 'import sys,json; print(json.load(sys.stdin)["task_id"])')"

echo "== manage: poll task status"
for _ in 1 2 3 4 5 6 7 8 9 10; do
  STATUS_RESP="$(curl -fsS "${MANAGE}/api/manage/tasks/${TASK_ID}" "${common_headers[@]}")"
  STATUS="$(echo "${STATUS_RESP}" | python3 -c 'import sys,json; print(json.load(sys.stdin)["status"])')"
  echo "status=${STATUS}"
  if [[ "${STATUS}" == "completed" || "${STATUS}" == "failed" ]]; then
    break
  fi
  sleep 1
done

echo "== manage: stream task events (SSE)"
curl -fsSN "${MANAGE}/api/manage/tasks/${TASK_ID}/stream" "${common_headers[@]}" | head -n 8

echo "== manage: create task with webhook"
curl -fsS -X POST "${MANAGE}/api/manage/threads/${THREAD_ID}/tasks" \
  "${common_headers[@]}" \
  -H 'Content-Type: application/json' \
  -d '{
    "stream": false,
    "callback_url": "http://127.0.0.1:9999/mock-webhook",
    "input": {"messages":[{"role":"user","content":"回调测试"}]}
  }' >/tmp/open-harness-manage-webhook.json
python3 - <<'PY'
import json
v=json.load(open('/tmp/open-harness-manage-webhook.json'))
assert "task_id" in v
print("manage webhook submission ok")
PY

echo "acceptance done"
