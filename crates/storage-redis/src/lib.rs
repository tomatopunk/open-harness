//! Redis-backed cache / short-lived state (memory facts cache).

mod memory_cache;

pub use memory_cache::RedisMemoryStore;
