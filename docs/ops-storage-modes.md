# 运维：各 `storage.mode` 键空间与备份单位

单一部署只启用一个 `storage.mode`。以下为审计、备份与灾难恢复时的**命名空间速查**。

## 通用

- 装配入口：`storage-registry::build_runtime_storage` → `StorageRegistry`。
- 跨后端 parity 测试：`crates/storage-registry/tests/storage_parity.rs`（`local_fs` / `sqlite` / 内存 `S3` / Redis 需 `REDIS_URL` 且 `cargo test -- --ignored`）。

## `local_fs`

- **根路径**：`storage.local_fs.root`（默认 `.deer-flow/local-fs`）。
- **布局**：`LocalFsLayout` — thread 元数据、checkpoint、memory、artifacts、uploads 等均在该根下子目录；见 `crates/state-abstraction/src/local_fs.rs`。
- **备份**：打包整个 `local_fs_root` 目录；恢复时停写后解压回同一路径。

## `sqlite`

- **单位**：单个 SQLite 文件（`storage.sqlite.url` 或 `manage.sqlite_url`）。
- **表（示例）**：`checkpoints_step`, `checkpoints_latest`, `memory_facts`, `artifacts`, `thread_meta`, …（完整列表见 `SqliteRuntimeStore::migrate`）。
- **备份**：文件级拷贝（`VACUUM` 可选）；恢复 = 替换文件。

## `postgres`

- **单位**：一个 PostgreSQL 数据库 / schema（连接串 `storage.postgres.url`）。
- **表**：与 SQLite 语义对齐的 `checkpoints_*`, `memory_facts`, …。
- **备份**：`pg_dump` / 逻辑卷快照；按租户策略执行。

## `redis`

- **前缀**：`oh:rt:v1:`（见 `crates/storage-redis/src/redis_runtime.rs` 常量 `P`）。
- **典型键**：
  - Thread meta: `oh:rt:v1:tmeta:{uuid}`
  - Checkpoint latest: `oh:rt:v1:cp:latest:{thread_uuid}:{run_uuid}`
  - Checkpoint step: `oh:rt:v1:cp:step:{thread_uuid}:{run_uuid}:{step}`
  - Memory: `oh:rt:v1:mem:{thread_uuid}`，索引集合 `oh:rt:v1:mem:threads`
  - 其余域（skills、tools、manage_tasks、…）均以前缀 `oh:rt:v1:` 开头。
- **备份**：RDB/AOF 或按前缀扫描导出；恢复需相同 key 语义。

## `s3`（及兼容 object store）

- **Bucket**: `storage.s3.bucket`
- **前缀**：`{storage.s3.prefix}/runtime/v1/`（`S3RuntimeStore::p`），例如 `open-harness/runtime/v1/...`。
- **可选配置**: `storage.s3.region`, `storage.s3.endpoint` (用于 S3 兼容存储)
- **对象路径示例**：`checkpoints/step/{thread}/{run}/{step}.json`, `memory/{thread}.json`, `artifacts/{thread}/{name}`。
- **备份**：版本控制 / 跨区域复制 / bucket 快照策略；恢复 = 还原前缀下对象。

## `manage.threads_root`

- **已弃用于 durable I/O**：manage 不再将 `threads_root` 作为上传/工件/删除线程的契约路径；凡属持久状态一律 `storage.mode` + `StorageRegistry`（与 [`存储引擎设计.md`](./存储引擎设计.md) §6 C1 一致）。
- 字段仍保留于 YAML 以便旧配置与外部工具（例如仅挂载卷路径的脚本）兼容；**不要**依赖它作为第二套存储事实来源。
