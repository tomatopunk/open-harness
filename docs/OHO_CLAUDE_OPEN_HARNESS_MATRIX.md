# OHO、Claude 与 Open Harness 功能 / 架构矩阵

## 目的

本文档用于把本轮重构后的 Open Harness 与两条参考方向并排说明：

- **OHO**: 这里指本次重构计划中吸收的 oh-my-openagent 风格结论，重点是开放扩展、插件化、hook 化、子 agent 编排。
- **Claude**: 这里指本次重构计划中吸收的 Claude Code 风格结论，重点是 session、状态控制、流式工具执行、安全链、记忆闭环。
- **Open Harness**: 当前仓库已经落地的实现，不写目标态想象。

> 说明：架构 closeout 以 `docs/ARCHITECTURE_V2_CLOSEOUT.md`、`docs/lifecycle_phase.md`、`docs/state_machine.md`、`docs/SECURITY_EVOLUTION.md`、计划、notepad 和当前代码为准。

## 总结

Open Harness 现在的落点，不是照搬 OHO，也不是照搬 Claude。它更像是一个混合内核：

- 在扩展边界上更接近 OHO，保留了 plugin、MCP、hook、开放工具总线这些开放面。
- 在引擎核心上更接近 Claude，已经把 session、显式状态机、流式工具总线、安全执行链、记忆闭环做成了明确边界。
- 当前差距主要不在主干契约，而在能力深度。尤其是六类工具族虽然都进入了统一 runtime 契约，但不等于每一类都已经达到 Claude 那种产品深度。

## 功能 / 架构矩阵

| 维度 | OHO 参考结论 | Claude 参考结论 | Open Harness 当前落点 | 现状判断 | 差距 / 备注 |
|---|---|---|---|---|---|
| 会话模型 | 偏开放编排，强调子 agent / fork 场景与上层调度组合。 | 把 session 作为核心运行单元，工具、权限、记忆都绑定到 session / thread 身份。 | 已实现显式 `SessionCore`，支持 `create / attach / fork / close`，父子会话继承上下文和策略快照。落点：`crates/state-abstraction/src/session_core.rs`，内核持有 `session_core` 于 `crates/agent-kernel/src/kernel.rs`。 | **已实现** | 当前 `SessionCore` 是内存域模型，适合内核边界与测试认证；不是完整的跨进程持久会话服务。 |
| 状态机 | 更偏编排器与 hook 驱动阶段，不一定强调一个统一显式 FSM。 | 明确运行状态和状态转移，工具执行也要纳入运行时控制。 | 已实现 `KernelStateMachine`，状态为 `Created / Initialized / Running / Stopped`，工具执行是 `Running` 内的自转移事件。落点：`crates/agent-kernel/src/state_machine.rs`，补充文档在 `docs/state_machine.md`。 | **已实现** | 状态机已经明确，但粒度仍偏内核级，不是更细的 planner / turn / subagent 多层状态图。 |
| 流式 runtime / 工具总线 | 偏开放分发与可插拔工具执行链，hook 可以包裹前后置行为。 | 明确 request、stream、finalize、error 的流式工具执行语义。 | 已实现共享 `StreamingToolRuntime` 契约与内核执行入口。事件面为 `Request / StreamChunk / Finalize / Error`。落点：`crates/agent-ports/src/streaming_runtime.rs` 与 `crates/agent-kernel/src/streaming_runtime.rs`。 | **已实现** | 当前主干契约已到位，强项在统一总线，不在某个单独工具族的产品化深度。 |
| 文件 / Bash / Agent / MCP / Skill / Web 能力面 | OHO 风格强调开放工具面和子 agent 编排，可通过插件和外部能力接入。 | Claude 风格强调统一工具体验，工具族之间共享相似执行控制和安全前置。 | `ToolAdapterKind` 已显式覆盖 `File`、`Bash`、`AgentFork`、`Mcp`、`Skill`、`Web` 六类。内核注册对应 adapter 类型，E2E `capability_matrix.rs` 明确检查共享总线覆盖。 | **主干已实现，深度部分** | 当前可以诚实地说“六类能力已进入统一 runtime 契约并进入验收矩阵”，但不能说六类都已达到同等成熟度或完整产品深度。 |
| 安全链 | 更像开放注入点，策略和 hook 可以外接。 | 更像内核前置链，权限决策、命令风险、沙箱、审计一起串起来。 | 已实现 `Policy Check -> Bash Classifier -> Process Sandbox Profile -> Audit`。执行时注入 `ToolExecutionSecurityContext`，高风险 bash 会阻断并落审计。落点：`crates/agent-kernel/src/security.rs`、`crates/agent-kernel/src/streaming_runtime.rs`、`crates/state-abstraction/src/traits.rs`、`docs/SECURITY_EVOLUTION.md`。 | **已实现** | 当前沙箱是进程级 profile 选择，不是容器级或虚拟机级隔离。策略默认仍是 allow-by-default，这点与更强约束的系统不同。 |
| 记忆 / 压缩 / 长期记忆 | OHO 参考更像开放记忆注入与 compaction hook，可把能力挂到编排链。 | Claude 参考更强调长会话闭环，压缩和长期记忆要服务 session 连续性。 | 已有分段上下文与压缩配置，包含阈值触发、里程碑快照、recent / working / archived 分层，以及向量 / 稀疏检索接口。落点：`crates/state-abstraction/src/lib.rs` 导出的 `CompressionDecision`、`SegmentedContext`、`retrieve_segmented_context` 等，配置在 `crates/state-abstraction/src/memory/config.rs`。 | **已实现核心** | 当前代码证明了分段、压缩、检索主干已存在，但它更像内核侧记忆系统，不应表述成已经有独立外部长期记忆平台。 |
| 扩展 / 插件 / hook 架构 | 强插件化、强 hook 化，是 OHO 参考最明显的方向之一。 | 更偏一体化运行时，扩展点存在，但不像开放插件平台那样是第一身份。 | 已实现 plugin 生命周期 `discover / load / initialize / start / stop / unload`，并记录状态快照；内核保留生命周期阶段与 hook 系统。落点：`crates/plugin-system/src/plugin.rs`、`crates/plugin-system/src/manager.rs`、`docs/lifecycle_phase.md`。 | **已实现** | Open Harness 在扩展架构上更接近 OHO，但现阶段插件生态规模仍小，能力更多体现在边界设计而不是生态数量。 |
| MCP 与外部能力接入 | OHO 参考强调开放协议和外部能力编排。 | Claude 参考强调把外部工具纳入统一执行与安全控制。 | MCP bridge 已独立成桥接层，且和工具缓存、生命周期分离；MCP 也进入统一工具总线与 E2E 回归。落点：`crates/mcp-bridge/src/manager.rs`、`e2e/src/tests/capability_matrix.rs`。 | **已实现** | 当前优势是边界清晰与回归覆盖，不应扩写成“完整外部生态平台”已完成。 |
| 安全与记忆闭环验收 | OHO 参考更强调开放编排能力本身。 | Claude 参考更强调整条执行链闭环，从 session 到工具到记忆都要贯通。 | 已有 `engine_certification` 与 `capability_matrix` 作为 release gate 风格验收，明确覆盖 session、FSM、runtime、security、memory。落点：`e2e/src/tests/engine_certification.rs`、`e2e/src/tests/capability_matrix.rs`。 | **已实现** | 当前文档和测试能证明“链路闭环”，但不等于所有用户场景都已有完整产品级 UX。 |
| 当前 Open Harness 总体定位 | 开放、插件优先、外部能力优先。 | 一体化引擎、session 驱动、工具安全和记忆闭环优先。 | 现在的 Open Harness 是“开放扩展边界 + 明确引擎内核”的混合体。 | **有意不同** | 它不是 OHO clone，也不是 Claude clone。它的独特点是把 OHO 式开放边界和 Claude 式引擎主干放到同一个 Rust 内核里。 |

## 当前 Open Harness 落点速查

| 主题 | 当前落点 |
|---|---|
| Session Core | `crates/state-abstraction/src/session_core.rs` |
| Kernel State Machine | `crates/agent-kernel/src/state_machine.rs` |
| Streaming Tool Runtime Contract | `crates/agent-ports/src/streaming_runtime.rs` |
| Kernel-owned Tool Runtime Path | `crates/agent-kernel/src/streaming_runtime.rs` |
| Runtime Security Chain | `crates/agent-kernel/src/security.rs` |
| Plugin Lifecycle | `crates/plugin-system/src/plugin.rs`, `crates/plugin-system/src/manager.rs` |
| MCP Lifecycle and Cache Boundary | `crates/mcp-bridge/src/manager.rs` |
| Memory / Compression / Retrieval | `crates/state-abstraction/src/lib.rs`, `crates/state-abstraction/src/memory/config.rs` |
| Capability Acceptance Matrix | `e2e/src/tests/capability_matrix.rs` |
| Engine Closure Certification | `e2e/src/tests/engine_certification.rs` |

## 结论

如果只看这轮实现结果，Open Harness 已经具备下列事实：

1. **核心引擎边界已经落地**：session、状态机、流式工具总线、安全链、记忆闭环都不是口号，而是代码边界。
2. **开放扩展面仍然保留**：plugin、MCP、skill、web、agent fork 没有被做成只能内建的私有路径。
3. **真正的差距在能力厚度**：当前主干契约和验收矩阵已经成立，但很多能力还处在“内核已可证明”阶段，不应直接对外表述成已经与 Claude 或 OHO 的完整产品面等价。

这也是本文对矩阵里“已实现 / 部分 / 不同”判断的标准。
