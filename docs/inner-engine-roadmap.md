# Inner engine roadmap

## 1. Status

### 1.1 已完成

| Item | Notes |
|------|-------|
| Storage registry | Unified `StorageRegistry` per `storage.mode`. |

### 1.2 刻意不做

- Out of scope for this document: external product launch dates.

### 1.3 迁宿主

- Orchestrator hosts the inner loop; gateway remains transport-only.

## 2. Plan R0

- Baseline inner loop contracts frozen.

## 3. Plan R1

- Replay and resume semantics covered by tests.

## 4. Plan R2

- Hardening passes.

## 5. Plan R3

- Integration profiles.

## 6. Plan R4

- Release polish.

## 7. Release Gate

- `make pre-commit` green on main.
