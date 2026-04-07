# Architecture Refactor Closeout

## 当前落地范围

本轮架构重构已经把计划中的核心内核边界落到了当前仓库：

- `agent-kernel` 负责 lifecycle、state machine、streaming runtime、安全链与 session 接入。
- `state-abstraction` 提供 session core、memory persistence、segmented memory compression / retrieval。
- `plugin-system`、`mcp-bridge`、`unified-config` 保持开放扩展边界，并由 kernel 在运行时装配。

## 已关闭的兼容层

- 运行时不再保留 `legacy / unified / auto` 模式分支。
- `KernelConfig::resolve_runtime()` 只走当前 unified runtime 解析路径。
- 文档与验收口径以 checked-in `config.yaml` + `governance/` + `extensions_config.json` 为准。

## 发布阻塞项已纳入验收

- runtime 配置必须能解析出可启动的默认模型。
- kernel 初始化 / 启动 / 停止必须能在当前配置下通过。
- segmented memory 不能在 reload 时丢失已压缩状态，也不能无意义膨胀 compression log。
- E2E acceptance wrapper 必须覆盖 hook 链、kernel lifecycle、session lineage、security / memory 闭环。

## 当前 release gate

- `cargo test --workspace`
- `cargo test --manifest-path e2e/Cargo.toml --tests`
- 手动 kernel startup sanity run（使用 checked-in runtime 配置）

## 仍然明确不在本轮范围内

- 重新引入任何预 1.0 兼容分支
- 为已存在统一 runtime 再维护第二套 legacy 文档口径
- 把当前内核边界描述成完整产品化平台能力
