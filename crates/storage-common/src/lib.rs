//! Shared non-domain helpers for `storage-*` crates (SCAN loops, chunking patterns).

use redis::aio::ConnectionManager;

/// Collect all keys matching `pattern` via Redis `SCAN` (not `KEYS`).
pub async fn redis_scan_match(
    mut conn: ConnectionManager,
    pattern: &str,
) -> Result<Vec<String>, redis::RedisError> {
    let mut cursor = 0u64;
    let mut out = Vec::new();
    loop {
        let (next, keys): (u64, Vec<String>) = redis::cmd("SCAN")
            .arg(cursor)
            .arg("MATCH")
            .arg(pattern)
            .arg("COUNT")
            .arg(500u32)
            .query_async(&mut conn)
            .await?;
        out.extend(keys);
        cursor = next;
        if cursor == 0 {
            break;
        }
    }
    Ok(out)
}
