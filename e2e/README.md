# Open Harness E2E Tests

端到端测试套件，用于验证 Open Harness 的完整功能。

## 快速开始

### 一行命令运行所有 E2E 测试

```bash
make e2e
```

或者直接使用 Cargo：

```bash
cargo test --manifest-path e2e/Cargo.toml --tests
```

## 测试套件

### 可用的测试

- `gateway` - 网关相关测试
- `mcp` - MCP 桥接测试
- `agent_loop` - Agent 循环测试
- `engine_certification` - 引擎总验收（session → FSM → runtime → security → memory）

### 运行单个测试

```bash
# 运行 gateway 测试
cargo test --manifest-path e2e/Cargo.toml gateway

# 运行 mcp 测试
cargo test --manifest-path e2e/Cargo.toml mcp

# 运行 agent_loop 测试
cargo test --manifest-path e2e/Cargo.toml agent_loop

# 运行引擎总验收测试
cargo test --manifest-path e2e/Cargo.toml engine_certification
```

## 目录结构

```
e2e/
├── src/
│   ├── fixtures/     # 测试固件
│   ├── helpers/      # 测试辅助函数
│   ├── tests/        # 测试文件
│   └── lib.rs        # 库入口
├── Cargo.toml
└── README.md
```

## 开发说明

### 添加新测试

1. 在 `e2e/src/tests/` 目录下创建新的测试文件
2. 在 `e2e/Cargo.toml` 中添加测试配置（如果需要）
3. 运行 `make e2e` 验证测试

### 测试辅助函数

测试辅助函数位于 `e2e/src/helpers/` 目录，提供：
- 工作区根目录获取
- 工作区测试运行器
- 通用测试断言
