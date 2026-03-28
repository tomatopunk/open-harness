//! Conformance smoke: SQLite unified registry covers manage tasks + memory + MCP.

use chrono::Utc;
use config_runtime::AppConfig;
use state_abstraction::ManageTaskRecord;
use storage_registry::build_runtime_storage;
use uuid::Uuid;

#[tokio::test]
async fn sqlite_registry_manage_task_and_memory_and_mcp() {
    let mut cfg = AppConfig::default();
    cfg.storage.mode = "sqlite".into();
    cfg.storage.sqlite_url = Some("sqlite::memory:".into());

    let bundle = build_runtime_storage(&cfg).await.expect("build");
    let reg = &bundle.registry;

    let mcp = serde_json::json!({"a": {"url": "http://x"}});
    reg.mcp_config.put_mcp_servers(&mcp).await.expect("mcp");
    assert_eq!(reg.mcp_config.get_mcp_servers().await.expect("get"), mcp);

    let tid = Uuid::new_v4();
    reg.memory.append_fact(tid, "hello").await.expect("fact");
    let facts = reg.memory.list_facts(tid).await.expect("list");
    assert_eq!(facts, vec!["hello".to_string()]);
    let ids = reg.memory.list_thread_ids_with_memory().await.expect("ids");
    assert!(ids.contains(&tid));

    let now = Utc::now();
    let rec = ManageTaskRecord {
        task_id: "t1".into(),
        thread_id: tid.to_string(),
        status: "queued".into(),
        output_chunks: vec![],
        error: None,
        callback_url: None,
        stream: false,
        client_task_id: None,
        tenant_id: "tenant".into(),
        user_id: "user".into(),
        created_at: now,
        updated_at: now,
        version: 1,
    };
    reg.manage_tasks.upsert_task(&rec).await.expect("task");
    let got = reg.manage_tasks.get_task("t1").await.expect("get task").expect("some");
    assert_eq!(got.task_id, "t1");
}
