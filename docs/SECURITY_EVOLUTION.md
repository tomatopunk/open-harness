# Open Harness Security Evolution Rules

## Purpose

This document defines the long-term versioned hardening rules for the engine security chain that guards tool execution across session policy, runtime evaluation, sandbox selection, and durable audit trails.

## Security Contract Surface

Every certified tool execution must preserve this order:

1. Attach an active session.
2. Resolve effective session policy.
3. Evaluate the runtime security chain.
4. Inject `ToolExecutionSecurityContext` onto the request before adapter invocation.
5. Persist a durable audit record for blocked, startup-error, and finalized outcomes.
6. Keep kernel FSM transitions observable through `ToolExecutionStarted` and `ToolExecutionFinished`.
7. Reuse the thread/session identity for downstream context and memory retrieval.

Any future change that skips, reorders, or partially bypasses those steps is a breaking security change.

## Versioning Rules

### Patch hardening (`x.y.Z`)

Allowed when all of the following are true:

- Existing allow/deny defaults remain compatible.
- Audit schema remains readable by the current code.
- Sandbox profile selection only becomes stricter for already-high-risk cases.
- Existing certified E2E scenarios continue to pass unchanged.

Examples:

- Expanding audit metadata.
- Improving bash classifier wording without changing allow/block semantics.
- Tightening observability around blocked executions.

### Minor hardening (`x.Y.0`)

Required when behavior becomes stricter but can be rolled out compatibly with opt-in or staged defaults.

Rules:

- Introduce the new policy behind an explicit version, flag, or governance setting.
- Document migration guidance before enabling the stricter mode by default.
- Add or update E2E certification scenarios proving the staged path and the default hardened path.

Examples:

- New medium-risk bash classifications.
- New restricted sandbox profiles for additional adapter kinds.
- Session-policy keys that refine execution scope.

### Major hardening (`X.0.0`)

Required when compatibility cannot be preserved.

Triggers:

- Denying requests that were previously allowed by default.
- Removing audit fields or changing audit semantics incompatibly.
- Replacing sandbox profile contracts.
- Requiring new mandatory policy fields on sessions or tools.

Major changes must include an explicit migration guide, an audit schema transition plan, and new certification evidence.

## Staged Hardening Policy

All substantive hardening changes should move through these stages:

1. **Observe** - add audit-only visibility, keep decisions unchanged.
2. **Warn** - surface reasons and candidate policy decisions without blocking.
3. **Constrain** - enable stricter behavior behind opt-in policy or version gates.
4. **Enforce** - promote the hardened behavior to the default.
5. **Retire** - remove transitional paths once certified adoption is complete.

Do not skip directly from observe to enforce unless the issue is an active critical vulnerability.

## Audit and Schema Evolution Rules

- Audit records must remain append-only.
- New fields must be additive before they become required.
- Readers must tolerate older records during at least one staged-hardening cycle.
- Session ID, thread ID, policy action, sandbox profile, and terminal outcome are mandatory invariants.

## Certification Gate

Security hardening is not complete until these automated checks pass:

- `cargo test --workspace`
- `cargo test --manifest-path e2e/Cargo.toml --tests`
- The engine certification suite covering session, FSM, adapter bus, security chain, and memory closure

## Change Checklist

- Update this document when changing policy semantics or version gates.
- Add or refresh E2E certification scenarios for the new hardening stage.
- Preserve durable audit compatibility unless making an intentional major change.
- Record the decision in `.sisyphus/notepads/open-harness-architecture-refactor/decisions.md`.
