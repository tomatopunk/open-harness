//! Redis-backed cache / short-lived state (memory facts cache).

mod manage_task;
mod memory_cache;
mod redis_runtime;

pub use manage_task::RedisManageTaskStore;
pub use memory_cache::RedisMemoryStore;
pub use redis_runtime::RedisRuntimeStore;
