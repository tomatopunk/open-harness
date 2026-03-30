//! Atomic memory storage with cache invalidation.

use crate::memory_document::MemoryDocument;
use crate::traits::{MemoryStore, StateError};
use chrono::Utc;
use uuid::Uuid;

/// Atomic memory store wrapper with cache support.
pub struct AtomicMemoryStore<S: MemoryStore> {
    inner: S,
    cache: std::sync::Arc<tokio::sync::RwLock<std::collections::HashMap<Uuid, CachedDocument>>>,
    enable_cache: bool,
}

struct CachedDocument {
    document: MemoryDocument,
    #[allow(dead_code)]
    cached_at: chrono::DateTime<Utc>,
    version: u64,
}

impl<S: MemoryStore> AtomicMemoryStore<S> {
    /// Create a new atomic memory store.
    pub fn new(inner: S, enable_cache: bool) -> Self {
        Self {
            inner,
            cache: std::sync::Arc::new(tokio::sync::RwLock::new(std::collections::HashMap::new())),
            enable_cache,
        }
    }

    /// Save memory atomically.
    pub async fn save_memory_atomic(
        &self,
        thread_id: Uuid,
        doc: &MemoryDocument,
    ) -> Result<(), StateError> {
        // Save to underlying store
        self.inner.save_memory_document(thread_id, doc).await?;

        // Update cache
        if self.enable_cache {
            let version = {
                let cache = self.cache.read().await;
                cache.get(&thread_id).map(|c| c.version + 1).unwrap_or(0)
            };

            let mut cache = self.cache.write().await;
            cache.insert(
                thread_id,
                CachedDocument { document: doc.clone(), cached_at: Utc::now(), version },
            );
        }

        Ok(())
    }

    /// Invalidate cache for a thread.
    pub async fn invalidate_cache(&self, thread_id: Uuid) -> Result<(), StateError> {
        if self.enable_cache {
            let mut cache = self.cache.write().await;
            cache.remove(&thread_id);
        }
        Ok(())
    }

    /// Load memory with cache support.
    pub async fn load_memory(&self, thread_id: Uuid) -> Result<MemoryDocument, StateError> {
        // Try cache first
        if self.enable_cache {
            if let Some(cached) = self.cache.read().await.get(&thread_id) {
                // Cache hit - return cached document
                return Ok(cached.document.clone());
            }
        }

        // Cache miss - load from storage
        let doc = self.inner.load_memory_document(thread_id).await?;

        // Update cache
        if self.enable_cache {
            let mut cache = self.cache.write().await;
            cache.insert(
                thread_id,
                CachedDocument { document: doc.clone(), cached_at: Utc::now(), version: 0 },
            );
        }

        Ok(doc)
    }

    /// Get cache statistics.
    pub async fn cache_stats(&self) -> CacheStats {
        let cache = self.cache.read().await;
        CacheStats { size: cache.len(), enabled: self.enable_cache }
    }

    /// Clear entire cache.
    pub async fn clear_cache(&self) -> Result<(), StateError> {
        if self.enable_cache {
            let mut cache = self.cache.write().await;
            cache.clear();
        }
        Ok(())
    }
}

/// Cache statistics.
#[derive(Debug, Clone)]
pub struct CacheStats {
    pub size: usize,
    pub enabled: bool,
}

impl<S: MemoryStore + Clone> Clone for AtomicMemoryStore<S> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            cache: self.cache.clone(),
            enable_cache: self.enable_cache,
        }
    }
}

#[cfg(test)]
#[allow(clippy::all)]
mod tests {
    use super::*;
    use crate::memory_document::{Fact, FactCategory};
    use async_trait::async_trait;

    // Mock store for testing
    struct MockMemoryStore {
        storage:
            std::sync::Arc<tokio::sync::RwLock<std::collections::HashMap<Uuid, MemoryDocument>>>,
    }

    impl MockMemoryStore {
        fn new() -> Self {
            Self {
                storage: std::sync::Arc::new(tokio::sync::RwLock::new(
                    std::collections::HashMap::new(),
                )),
            }
        }
    }

    #[async_trait]
    impl MemoryStore for MockMemoryStore {
        async fn load_memory_document(
            &self,
            thread_id: Uuid,
        ) -> Result<MemoryDocument, StateError> {
            let storage = self.storage.read().await;
            Ok(storage.get(&thread_id).cloned().unwrap_or_default())
        }

        async fn save_memory_document(
            &self,
            thread_id: Uuid,
            doc: &MemoryDocument,
        ) -> Result<(), StateError> {
            let mut storage = self.storage.write().await;
            storage.insert(thread_id, doc.clone());
            Ok(())
        }

        async fn list_thread_ids_with_memory(&self) -> Result<Vec<Uuid>, StateError> {
            let storage = self.storage.read().await;
            Ok(storage.keys().cloned().collect())
        }
    }

    #[tokio::test]
    async fn test_atomic_save_and_cache() {
        let mock = MockMemoryStore::new();
        let store = AtomicMemoryStore::new(mock, true);

        let thread_id = Uuid::new_v4();
        let mut doc = MemoryDocument::default();
        doc.add_fact(Fact::new("Test".to_string(), FactCategory::Knowledge, 0.9, "t1".to_string()));

        // Save atomically
        store.save_memory_atomic(thread_id, &doc).await.unwrap();

        // Load from cache
        let loaded = store.load_memory(thread_id).await.unwrap();
        assert_eq!(loaded.facts.len(), 1);

        // Check cache stats
        let stats = store.cache_stats().await;
        assert_eq!(stats.size, 1);
        assert!(stats.enabled);
    }

    #[tokio::test]
    async fn test_cache_invalidation() {
        let mock = MockMemoryStore::new();
        let store = AtomicMemoryStore::new(mock, true);

        let thread_id = Uuid::new_v4();
        let doc = MemoryDocument::default();

        // Save and cache
        store.save_memory_atomic(thread_id, &doc).await.unwrap();

        // Invalidate
        store.invalidate_cache(thread_id).await.unwrap();

        // Check cache is empty
        let stats = store.cache_stats().await;
        assert_eq!(stats.size, 0);
    }

    #[tokio::test]
    async fn test_cache_clear() {
        let mock = MockMemoryStore::new();
        let store = AtomicMemoryStore::new(mock, true);

        // Add multiple entries
        for _i in 0..5 {
            let thread_id = Uuid::new_v4();
            let doc = MemoryDocument::default();
            store.save_memory_atomic(thread_id, &doc).await.unwrap();
        }

        // Clear cache
        store.clear_cache().await.unwrap();

        let stats = store.cache_stats().await;
        assert_eq!(stats.size, 0);
    }
}
