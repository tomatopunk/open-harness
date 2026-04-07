# AGENT-PORTS CRATE

## OVERVIEW

Port abstraction layer, providing 20 files defining various port interfaces and implementations.

## STRUCTURE

```
agent-ports/
├── src/
│   ├── lib.rs
│   ├── ports.rs           # Port definitions
│   ├── thread_state.rs    # Thread state
│   ├── memory/           # Memory ports (4 files)
│   └── ... (20 files total)
└── Cargo.toml
```

## WHERE TO LOOK

| Task | Location |
|------|----------|
| Port definitions | ports.rs |
| Thread state | thread_state.rs |
| Memory ports | memory/ |
