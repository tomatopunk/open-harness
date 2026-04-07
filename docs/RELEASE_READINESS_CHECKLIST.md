# Release Readiness Checklist

## Runtime configuration

- [x] `config.yaml` 可被 kernel 读取
- [x] `governance/models.yaml` 的 `default_model` 与当前 runtime 可解析模型一致
- [x] `extensions_config.json` 可装配 MCP server 清单

## Core verification

- [x] `cargo test --workspace`
- [x] `cargo test --manifest-path e2e/Cargo.toml --tests`
- [x] checked-in runtime configuration 的 kernel startup sanity run

## Final-wave regression guards

- [x] segmented memory reload 不会清空已压缩 working / archived 状态
- [x] compression log 不会因重复保存而无意义增长
- [x] late-phase E2E tests 覆盖真实行为而不是只检查构造成功
- [x] README 与 docs 不再保留已经删除的兼容/遗留口径
