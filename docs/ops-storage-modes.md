# 运维：各 `storage.mode` 键空间与备份单位

单一部署只启用一个 `storage.mode`。以下为审计、备份与灾难恢复时的**命名空间速查**。

## 通用

- 装配入口：`storage-registry::build_runtime_storage` → `StorageRegistry`。
- 跨后端 parity 测试：`crates/storage-registry/tests/storage_parity.rs`（`local_fs` / `sqlite` / 内存 `S3` / Redis 需 `REDIS_URL` 且 `cargo test -- --ignored`）。

## `local_fs`

- **根路径**：`storage.local_fs_root`（默认 `.deer-flow/local-fs`）。
- **布局**：`LocalFsLayout` — thread 元数据、checkpoint、memory、artifacts、uploads 等均在该根下子目录；见 `crates/state-abstraction/src/local_fs.rs`。
- **备份**：打包整个 `local_fs_root` 目录；恢复时停写后解压回同一路径。

## `sqlite`

- **单位**：单个 SQLite 文件（`storage.sqlite_url` 或 `manage.sqlite_url`）。
- **表（示例）**：`checkpoints_step`, `checkpoints_latest`, `memory_facts`, `artifacts`, `thread_meta`, …（完整列表见 `SqliteRuntimeStore::migrate`）。
- **备份**：文件级拷贝（`VACUUM` 可选）；恢复 = 替换文件。

## `postgres`

- **单位**：一个 PostgreSQL 数据库 / schema（连接串 `storage.postgres_url`）。
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

- **前缀**：`{s3_prefix}/runtime/v1/`（`S3RuntimeStore::p`），例如 `open-harness/runtime/v1/...`。
- **对象路径示例**：`checkpoints/step/{thread}/{run}/{step}.json`, `memory/{thread}.json`, `artifacts/{thread}/{name}`。
- **备份**：版本控制 / 跨区域复制 / bucket 快照策略；恢复 = 还原前缀下对象。

## `manage.threads_root`

- **仅**本地线程工作区布局与兼容（例如 Docker 卷挂载），**不是**第二套 Registry 存储契约； durable 状态一律走 `StorageRegistry`。详见根目录 `README.md` 与 `config.example.yaml` 注释。
