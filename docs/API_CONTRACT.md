# Open Harness API Contract (Frozen v0)

本文冻结当前迭代的统一接口合同，后续阶段仅做向后兼容扩展。

## 配置与存储模式

- 启动配置：默认读取仓库根目录 `config.yaml`，可通过 `OPEN_HARNESS_CONFIG_PATH` 覆盖。
- 配置优先级：`config.yaml` -> `OPEN_HARNESS_*` 环境变量覆盖。
- 存储模式：`storage.mode` 支持 `local_fs|sqlite|postgres|redis|s3`。
- 开发默认：`local_fs`，目录根由 `storage.local_fs_root` 指定。
- `local_fs` 目录约定：
  - `config/`：MCP、skills 等配置快照
  - `tasks/`：任务建议与任务态文件
  - `threads/`：线程与上传/产物数据
  - `memory/`：长期记忆快照

## OpenAI 兼容

- `GET /v1/models`
  - 返回 OpenAI `list` 结构，`data[].id` 可被 Cursor/OpenCode 直接选择。
  - 模型列表来自 `config.yaml` 的 `models[]`。
- `POST /v1/chat/completions`
  - 入参最小子集：`model/messages/temperature/max_tokens/stream/user`
  - 行为：
    - `stream=false`：返回 `chat.completion`
    - `stream=true`：返回 SSE `chat.completion.chunk` + `[DONE]`
  - 会话映射：`user -> thread_id`（服务端维护）

## 管理面（/api）

- Models
  - `GET /api/models`
  - `GET /api/models/{model_name}`
- MCP
  - `GET /api/mcp/config`
  - `PUT /api/mcp/config`
- Memory
  - `GET /api/memory`
  - `POST /api/memory/reload`
  - `GET /api/memory/config`
  - `GET /api/memory/status`
- Skills
  - `GET /api/skills`
  - `GET /api/skills/{skill_name}`
  - `PUT /api/skills/{skill_name}`
- Threads / Files
  - `DELETE /api/threads/{thread_id}`
  - `POST /api/threads/{thread_id}/uploads`
  - `GET /api/threads/{thread_id}/uploads/list`
  - `DELETE /api/threads/{thread_id}/uploads/{filename}`
  - `GET /api/threads/{thread_id}/artifacts/{path}`
  - `POST /api/threads/{thread_id}/suggestions`
- Agents / User Profile
  - `GET|POST /api/agents`
  - `GET /api/agents/check?name=...`
  - `GET|PUT|DELETE /api/agents/{name}`
  - `GET|PUT /api/user-profile`
- Channels
  - `GET /api/channels/`
  - `POST /api/channels/{name}/restart`

## 错误约定

- 网关/管理面统一返回 JSON 错误体：`{"error":"..."}`，必要时附带 `status`。
- 代理上游失败统一使用 `502 Bad Gateway`。
