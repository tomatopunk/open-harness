#!/usr/bin/env bash
# R2: fail CI if runtime dispatch bypasses Command IR (single entry: route_llm_output -> build_dispatch_plan_with_options).
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
RUNTIME_SRC="crates/agent-loop-runtime/src"

if ! grep -q 'agent_ports::build_dispatch_plan_with_options(out' "${RUNTIME_SRC}/dispatch.rs"; then
  echo "error: dispatch.rs must call agent_ports::build_dispatch_plan_with_options(out, ...)" >&2
  exit 1
fi

while IFS= read -r line; do
  [[ -z "${line}" ]] && continue
  file="${line%%:*}"
  if [[ "${file}" != "${RUNTIME_SRC}/dispatch.rs" ]]; then
    echo "error: forbidden Command IR call site outside dispatch.rs: ${line}" >&2
    exit 1
  fi
done < <(grep -Rsn 'agent_ports::build_dispatch_plan_with_options(out' "${RUNTIME_SRC}" --include='*.rs' || true)

# classify_llm_routing( — only turn_flow compatibility layer.
while IFS= read -r line; do
  [[ -z "${line}" ]] && continue
  file="${line%%:*}"
  if [[ "${file}" != *"/turn_flow.rs" ]]; then
    echo "error: classify_llm_routing must stay in turn_flow.rs only: ${line}" >&2
    exit 1
  fi
done < <(grep -Rsn 'classify_llm_routing(' "${RUNTIME_SRC}" --include='*.rs' || true)

echo "check_command_ir_guard: ok"
