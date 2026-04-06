# Open Harness 发布前验收清单

## 1. 架构与边界

- [ ] `docs/ARCHITECTURE_V2.md` 的“当前实现状态（收口同步）”与当前 workspace 结构一致。
- [ ] `docs/ARCHITECTURE_V2_CLOSEOUT.md` 的主链路依赖图仍与 `apps/kernel`、`agent-kernel`、`plugin-system`、`mcp-bridge`、`state-abstraction`、`unified-config` 的当前实现一致。
- [ ] `docs/BREAKING_CHANGES_MIGRATION.md` 仍可解释当前 `legacy / unified / auto` 迁移模式。
- [ ] 未把 Task 13 之后的未来引擎工作误标为“已完成”。

## 2. CI / 发布 gate

- [ ] `.github/workflows/ci.yml` 仍包含以下 gate：`cargo fmt --all -- --check`、`cargo lint`、`cargo build --workspace --all-targets`、`cargo xtest`、`cargo test --manifest-path e2e/Cargo.toml --tests`。
- [ ] e2e acceptance matrix 仍是单独的显式步骤，而不是隐式依赖。

## 3. 本地最终验证

- [ ] `cargo check --workspace`
- [ ] `cargo test --workspace`

## 4. 证据完整性

- [ ] `.sisyphus/evidence/task-12-closeout.md` 已记录本次 closeout 的文档、依赖摘要、验证命令与结论。
- [ ] `.sisyphus/notepads/open-harness-architecture-refactor/learnings.md` 已追加本次收口经验。
- [ ] `.sisyphus/notepads/open-harness-architecture-refactor/issues.md` 已追加本次收口遗留问题或环境限制。
- [ ] `.sisyphus/notepads/open-harness-architecture-refactor/decisions.md` 已追加本次收口决策。

## 5. 阻断条件

任一项出现以下情况时，不应视为可发布状态：

- `cargo check --workspace` 或 `cargo test --workspace` 失败。
- `docs/ARCHITECTURE_V2_CLOSEOUT.md` 的边界描述与当前 Cargo 依赖关系不一致。
- CI gate 与文档中的发布前 gate 不一致。
- evidence index 或 notepad 记录缺失，导致无法追溯本轮 closeout 结论。
