//! Optional timing smoke tests for cascade delete (not strict benchmarks; use `cargo test --release` for stable numbers).

use std::time::Instant;

use agent_ports::{RunId, StepSeq, ThreadId, ThreadState};
use chrono::Utc;
use config_runtime::AppConfig;
use graph_runtime_core::checkpoint_store::make_checkpoint;
use serde_json::json;
use state_abstraction::{StorageRegistry, ThreadLifecycleStore, ThreadMeta, ThreadMetaStore};
use storage_registry::build_runtime_storage;
use uuid::Uuid;

async fn setup_minimal_thread(reg: &StorageRegistry) -> Uuid {
    let tid = Uuid::new_v4();
    let now = Utc::now();
    let meta = ThreadMeta { thread_id: tid, created_at: now, updated_at: now, label: None };
    ThreadMetaStore::upsert_thread(&*reg.threads, &meta).await.expect("upsert_thread");
    let run_id = RunId::new_v4();
    let mut state = ThreadState::new(ThreadId::from(tid));
    state.step_seq = StepSeq(0);
    let cp = make_checkpoint(ThreadId::from(tid), run_id, state, json!({}));
    reg.checkpoints.save_checkpoint(&cp).await.expect("save_checkpoint");
    tid
}

#[tokio::test]
async fn perf_smoke_sqlite_cascade_delete_elapsed() {
    let mut cfg = AppConfig::default();
    cfg.storage.mode = "sqlite".into();
    cfg.storage.sqlite.url = Some("sqlite::memory:".into());
    let bundle = build_runtime_storage(&cfg).await.expect("build");
    let reg = &bundle.registry;
    let tid = setup_minimal_thread(reg).await;
    let start = Instant::now();
    ThreadLifecycleStore::delete_thread_cascade(&*reg.lifecycle, tid)
        .await
        .expect("delete_thread_cascade");
    let elapsed = start.elapsed();
    assert!(elapsed.as_secs() < 30, "sqlite cascade delete took {:?} (smoke threshold)", elapsed);
}

#[tokio::test]
#[ignore = "needs Redis; e.g. REDIS_URL=redis://127.0.0.1:6379 cargo test -p storage-registry --test perf_smoke -- --ignored"]
async fn perf_smoke_redis_cascade_delete_elapsed() {
    let url = std::env::var("REDIS_URL").unwrap_or_else(|_| "redis://127.0.0.1:6379".to_string());
    let mut cfg = AppConfig::default();
    cfg.storage.mode = "redis".into();
    cfg.storage.redis.url = Some(url);
    let bundle = build_runtime_storage(&cfg).await.expect("build");
    let reg = &bundle.registry;
    let tid = setup_minimal_thread(reg).await;
    let start = Instant::now();
    ThreadLifecycleStore::delete_thread_cascade(&*reg.lifecycle, tid)
        .await
        .expect("delete_thread_cascade");
    let elapsed = start.elapsed();
    assert!(
        elapsed.as_secs() < 120,
        "redis cascade delete took {:?} (smoke threshold; list+del may be slow)",
        elapsed
    );
}
