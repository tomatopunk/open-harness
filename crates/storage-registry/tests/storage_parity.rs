//! Cross-backend parity: lifecycle (cascade delete), uploads, manage_config, checkpoints list — same assertions on local_fs, sqlite, in-memory S3, and optionally Redis (`REDIS_URL`).

use std::sync::Arc;

use agent_ports::{RunId, StepSeq, ThreadId, ThreadState};
use chrono::Utc;
use config_runtime::AppConfig;
use graph_runtime_core::checkpoint_store::make_checkpoint;
use object_store::memory::InMemory;
use serde_json::json;
use state_abstraction::{
    ManageAppConfig, StorageRegistry, ThreadLifecycleStore, ThreadMeta, ThreadMetaStore,
};
use storage_registry::build_runtime_storage;
use storage_s3::S3RuntimeStore;
use uuid::Uuid;

async fn assert_lifecycle_upload_manage_checkpoint_parity(reg: &StorageRegistry) {
    let tid = Uuid::new_v4();
    let now = Utc::now();
    let meta = ThreadMeta { thread_id: tid, created_at: now, updated_at: now, label: None };
    ThreadMetaStore::upsert_thread(&*reg.threads, &meta).await.expect("upsert_thread");

    let mut mac = ManageAppConfig::default();
    mac.agents.insert("parity-agent".into(), json!({ "x": 1 }));
    reg.manage_config.put_manage_app_config(&mac).await.expect("put_manage_app_config");
    let got = reg.manage_config.get_manage_app_config().await.expect("get_manage_app_config");
    assert!(got.agents.contains_key("parity-agent"));

    reg.memory.append_fact(tid, "fact1").await.expect("append_fact");
    reg.uploads.put_upload(tid, "note.txt", b"hello").await.expect("put_upload");

    let run_id = RunId::new_v4();
    let mut state = ThreadState::new(ThreadId::from(tid));
    state.step_seq = StepSeq(0);
    let cp = make_checkpoint(ThreadId::from(tid), run_id, state, json!({}));
    reg.checkpoints.save_checkpoint(&cp).await.expect("save_checkpoint");

    let steps = reg
        .checkpoints
        .list_checkpoint_steps_for_run(ThreadId::from(tid), run_id)
        .await
        .expect("list_checkpoint_steps_for_run");
    assert_eq!(steps, vec![StepSeq(0)]);

    let at_step = reg
        .checkpoints
        .load_checkpoint_at_step(ThreadId::from(tid), run_id, StepSeq(0))
        .await
        .expect("load_checkpoint_at_step");
    assert!(at_step.is_some());

    ThreadLifecycleStore::delete_thread_cascade(&*reg.lifecycle, tid)
        .await
        .expect("delete_thread_cascade");

    assert!(ThreadMetaStore::get_thread(&*reg.threads, tid).await.is_err());
    assert!(reg.memory.list_facts(tid).await.expect("list_facts").is_empty());
    assert!(reg
        .uploads
        .list_upload_filenames(tid)
        .await
        .expect("list_upload_filenames")
        .is_empty());
    let steps_after = reg
        .checkpoints
        .list_checkpoint_steps_for_run(ThreadId::from(tid), run_id)
        .await
        .expect("list after delete");
    assert!(steps_after.is_empty());

    let mac2 = reg.manage_config.get_manage_app_config().await.expect("get manage after cascade");
    assert!(
        mac2.agents.contains_key("parity-agent"),
        "global manage_config must survive per-thread cascade"
    );
}

fn registry_from_s3(store: Arc<S3RuntimeStore>) -> StorageRegistry {
    StorageRegistry::new(
        store.clone() as Arc<dyn state_abstraction::ThreadMetaStore>,
        store.clone() as Arc<dyn state_abstraction::CheckpointStore>,
        store.clone() as Arc<dyn state_abstraction::ArtifactStore>,
        store.clone() as Arc<dyn state_abstraction::ThreadUploadStore>,
        store.clone() as Arc<dyn state_abstraction::MemoryStore>,
        store.clone() as Arc<dyn state_abstraction::SkillStore>,
        store.clone() as Arc<dyn state_abstraction::ToolRecordStore>,
        store.clone() as Arc<dyn state_abstraction::SubagentTaskStore>,
        store.clone() as Arc<dyn state_abstraction::SandboxExecutionStore>,
        store.clone() as Arc<dyn state_abstraction::ManageTaskStore>,
        store.clone() as Arc<dyn state_abstraction::McpConfigStore>,
        store.clone() as Arc<dyn state_abstraction::ManageConfigStore>,
        store.clone() as Arc<dyn state_abstraction::ThreadLifecycleStore>,
    )
}

#[tokio::test]
async fn parity_local_fs_lifecycle_upload_manage_checkpoint() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut cfg = AppConfig::default();
    cfg.storage.mode = "local_fs".into();
    cfg.storage.local_fs.root = dir.path().to_string_lossy().into();

    let bundle = build_runtime_storage(&cfg).await.expect("build");
    assert_lifecycle_upload_manage_checkpoint_parity(&bundle.registry).await;
}

#[tokio::test]
async fn parity_sqlite_lifecycle_upload_manage_checkpoint() {
    let mut cfg = AppConfig::default();
    cfg.storage.mode = "sqlite".into();
    cfg.storage.sqlite.url = Some("sqlite::memory:".into());

    let bundle = build_runtime_storage(&cfg).await.expect("build");
    assert_lifecycle_upload_manage_checkpoint_parity(&bundle.registry).await;
}

#[tokio::test]
async fn parity_s3_inmemory_lifecycle_upload_manage_checkpoint() {
    let store = Arc::new(InMemory::new());
    let s3 = Arc::new(S3RuntimeStore::new(store, "parity-suite"));
    let reg = registry_from_s3(s3);
    assert_lifecycle_upload_manage_checkpoint_parity(&reg).await;
}

#[tokio::test]
#[ignore = "needs Redis; e.g. REDIS_URL=redis://127.0.0.1:6379 cargo test -p storage-registry -- --ignored"]
async fn parity_redis_lifecycle_upload_manage_checkpoint() {
    let url = std::env::var("REDIS_URL").unwrap_or_else(|_| "redis://127.0.0.1:6379".to_string());
    let mut cfg = AppConfig::default();
    cfg.storage.mode = "redis".into();
    cfg.storage.redis.url = Some(url);

    let bundle = build_runtime_storage(&cfg).await.expect("build");
    assert_lifecycle_upload_manage_checkpoint_parity(&bundle.registry).await;
}
