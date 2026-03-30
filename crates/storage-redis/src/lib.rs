//! Redis-backed cache / short-lived state (memory facts cache).

mod manage_task;
mod memory_cache;
mod redis_runtime;
mod template_storage;

pub use manage_task::RedisManageTaskStore;
pub use memory_cache::RedisMemoryStore;
pub use redis_runtime::RedisRuntimeStore;
pub use template_storage::RedisTemplateStorage;
