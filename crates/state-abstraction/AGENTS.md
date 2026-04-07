# STATE-ABSTRACTION CRATE

## OVERVIEW

State abstraction layer, providing 19 files implementing state management, memory system, document management, and more.

## STRUCTURE

```
state-abstraction/
├── src/
│   ├── lib.rs
│   ├── session_core.rs    # Session core
│   ├── memory_system.rs   # Memory system
│   ├── memory_merge.rs    # Memory merge
│   ├── memory_voting.rs   # Memory voting
│   ├── memory/            # Memory module (3 files)
│   ├── memory/prompts/    # Prompts
│   └── ... (19 files total)
└── Cargo.toml
```

## WHERE TO LOOK

| Task | Location |
|------|----------|
| Session core | session_core.rs |
| Memory system | memory_system.rs |
| Memory merge | memory_merge.rs |
| Memory voting | memory_voting.rs |
| Memory module | memory/ |
