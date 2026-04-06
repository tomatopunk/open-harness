//! Unified memory system facade - integrates all components.

use agent_ports::ThreadId;
use uuid::Uuid;

use crate::memory_atomic::AtomicMemoryStore;
use crate::memory_document::{Fact, FactCategory, MemoryDocument};
use crate::memory_extractor::FactExtractor;
use crate::memory_injection::{inject_memory_to_prompt, MemoryInjectionConfig};
use crate::memory_manager::FactManager;
use crate::memory_merge::{MemoryMergeEngine, UserProfile};
use crate::memory_voting::MemoryVotingEngine;
use crate::traits::MemoryPersistence;
use thiserror::Error;

/// Configuration for the memory system.
#[derive(Debug, Clone)]
pub struct MemorySystemConfig {
    pub max_facts: usize,
    pub fact_confidence_threshold: f32,
    pub max_injection_tokens: usize,
}

impl Default for MemorySystemConfig {
    fn default() -> Self {
        Self { max_facts: 100, fact_confidence_threshold: 0.7, max_injection_tokens: 2000 }
    }
}

/// Statistics about memory operations.
#[derive(Debug, Clone, Default)]
pub struct MemorySystemStats {
    pub total_facts: usize,
    pub facts_extracted: usize,
    pub facts_merged: usize,
    pub conflicts_resolved: usize,
    pub cache_hits: usize,
}

#[derive(Debug, Error)]
pub enum MemorySystemError {
    #[error("{operation} failed: {source}")]
    StateOperation {
        operation: &'static str,
        #[source]
        source: crate::traits::StateError,
    },
    #[error("{operation} failed: {details}")]
    Runtime { operation: &'static str, details: String },
}

impl MemorySystemError {
    pub fn category(&self) -> crate::traits::StateErrorCategory {
        match self {
            Self::StateOperation { source, .. } => source.category(),
            Self::Runtime { .. } => crate::traits::StateErrorCategory::Runtime,
        }
    }

    fn state_operation(operation: &'static str, source: crate::traits::StateError) -> Self {
        Self::StateOperation { operation, source }
    }

    fn runtime(operation: &'static str, details: impl Into<String>) -> Self {
        Self::Runtime { operation, details: details.into() }
    }
}

/// Unified memory system for long-term fact management.
pub struct MemorySystem<S: MemoryPersistence> {
    config: MemorySystemConfig,
    voting_engine: MemoryVotingEngine,
    fact_manager: FactManager,
    store: AtomicMemoryStore<S>,
    stats: std::sync::Arc<tokio::sync::RwLock<MemorySystemStats>>,
}

impl<S: MemoryPersistence> MemorySystem<S> {
    /// Create a new memory system.
    pub fn new(config: MemorySystemConfig, store: S) -> Self {
        let voting_engine = MemoryVotingEngine::with_defaults();
        let fact_manager = FactManager::new(voting_engine.clone(), config.max_facts);
        let atomic_store = AtomicMemoryStore::new(store, true);

        Self {
            config,
            voting_engine,
            fact_manager,
            store: atomic_store,
            stats: std::sync::Arc::new(tokio::sync::RwLock::new(MemorySystemStats::default())),
        }
    }

    /// Get memory config.
    pub fn config(&self) -> &MemorySystemConfig {
        &self.config
    }

    /// Load memory for a thread.
    pub async fn load_memory(
        &self,
        thread_id: Uuid,
    ) -> Result<MemoryDocument, crate::traits::StateError> {
        let doc = self.store.load_memory(thread_id).await?;
        let mut stats = self.stats.write().await;
        stats.total_facts = doc.facts.len();
        Ok(doc)
    }

    /// Save memory for a thread.
    pub async fn save_memory(
        &self,
        thread_id: Uuid,
        doc: &MemoryDocument,
    ) -> Result<(), crate::traits::StateError> {
        self.store.save_memory_atomic(thread_id, doc).await
    }

    /// Extract facts from a conversation.
    pub async fn extract_facts(
        &self,
        llm_port: Box<dyn agent_ports::LLMPort>,
        thread_id: Uuid,
        conversation: &str,
    ) -> Result<FactExtractionResult, MemorySystemError> {
        // Load current memory
        let mut memory = self.load_memory(thread_id).await.map_err(|error| {
            MemorySystemError::state_operation("load memory for fact extraction", error)
        })?;

        // Extract facts using LLM
        let extractor = FactExtractor::new(llm_port, self.config.fact_confidence_threshold);

        let update = extractor.extract(&memory, conversation).await.map_err(|error| {
            MemorySystemError::runtime("extract facts from conversation", error)
        })?;
        let processed = extractor.process_update(update, thread_id.to_string());

        let facts_extracted = processed.new_facts.len();
        let facts_removed = processed.facts_to_remove.len();

        // Apply update
        processed.apply(&mut memory);

        // Manage facts (dedup, conflict resolution, eviction)
        let manage_stats = self.fact_manager.apply_to_document(&mut memory);

        // Save atomically
        self.save_memory(thread_id, &memory).await.map_err(|error| {
            MemorySystemError::state_operation("save extracted memory updates", error)
        })?;

        // Update stats
        {
            let mut stats = self.stats.write().await;
            stats.facts_extracted += facts_extracted;
            stats.conflicts_resolved += manage_stats.conflicts_resolved;
        }

        Ok(FactExtractionResult {
            facts_extracted,
            facts_removed,
            conflicts_resolved: manage_stats.conflicts_resolved,
            facts_evicted: manage_stats.facts_evicted,
            total_facts: memory.facts.len(),
        })
    }

    /// Merge memories from multiple threads.
    pub async fn merge_thread_memories(
        &self,
        llm_port: Box<dyn agent_ports::LLMPort>,
        user_id: String,
        thread_memories: &[(ThreadId, MemoryDocument)],
    ) -> Result<MergeResult, MemorySystemError> {
        let merge_engine = MemoryMergeEngine::new(llm_port, self.voting_engine.clone());

        let result = merge_engine
            .merge_threads(user_id, thread_memories, None)
            .await
            .map_err(|error| MemorySystemError::runtime("merge thread memories", error))?;

        {
            let mut stats = self.stats.write().await;
            stats.facts_merged = result.profile.merged_facts.len();
            stats.conflicts_resolved += result.conflicts_resolved;
        }

        Ok(MergeResult {
            profile: result.profile,
            facts_merged: result.facts_merged,
            conflicts_resolved: result.conflicts_resolved,
        })
    }

    /// Inject memory into a system prompt.
    pub fn inject_to_prompt(&self, system_prompt: &str, memory: &MemoryDocument) -> String {
        let config = MemoryInjectionConfig {
            max_tokens: self.config.max_injection_tokens,
            include_user_context: true,
            include_history: true,
            include_facts: true,
            facts_only: false,
        };

        inject_memory_to_prompt(system_prompt, memory, &config, &self.voting_engine)
    }

    /// Add a fact manually.
    pub async fn add_fact(
        &self,
        thread_id: Uuid,
        content: String,
        category: FactCategory,
        confidence: f32,
    ) -> Result<(), MemorySystemError> {
        let mut memory = self.load_memory(thread_id).await.map_err(|error| {
            MemorySystemError::state_operation("load memory for manual fact insertion", error)
        })?;

        let fact = Fact::new(content, category, confidence, thread_id.to_string());
        memory.add_fact(fact);

        // Manage facts
        self.fact_manager.apply_to_document(&mut memory);

        // Save
        self.save_memory(thread_id, &memory).await.map_err(|error| {
            MemorySystemError::state_operation("save memory after manual fact insertion", error)
        })
    }

    /// Get statistics.
    pub async fn stats(&self) -> MemorySystemStats {
        self.stats.read().await.clone()
    }

    /// Clear cache.
    pub async fn clear_cache(&self) -> Result<(), crate::traits::StateError> {
        self.store.clear_cache().await
    }
}

/// Result of fact extraction.
#[derive(Debug, Clone)]
pub struct FactExtractionResult {
    pub facts_extracted: usize,
    pub facts_removed: usize,
    pub conflicts_resolved: usize,
    pub facts_evicted: usize,
    pub total_facts: usize,
}

/// Result of memory merge.
#[derive(Debug, Clone)]
pub struct MergeResult {
    pub profile: UserProfile,
    pub facts_merged: usize,
    pub conflicts_resolved: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory_merge::test_utils::MockLLMPort;
    use crate::traits::{MemoryPersistence, MemoryStore, StateError, StateErrorCategory};
    use async_trait::async_trait;

    // Mock store for testing
    #[derive(Clone)]
    struct MockStore {
        storage:
            std::sync::Arc<tokio::sync::RwLock<std::collections::HashMap<Uuid, MemoryDocument>>>,
    }

    impl MockStore {
        fn new() -> Self {
            Self {
                storage: std::sync::Arc::new(tokio::sync::RwLock::new(
                    std::collections::HashMap::new(),
                )),
            }
        }
    }

    #[async_trait]
    impl MemoryPersistence for MockStore {
        async fn load_memory_document(
            &self,
            thread_id: Uuid,
        ) -> Result<MemoryDocument, crate::traits::StateError> {
            let storage = self.storage.read().await;
            Ok(storage.get(&thread_id).cloned().unwrap_or_default())
        }

        async fn save_memory_document(
            &self,
            thread_id: Uuid,
            doc: &MemoryDocument,
        ) -> Result<(), crate::traits::StateError> {
            let mut storage = self.storage.write().await;
            storage.insert(thread_id, doc.clone());
            Ok(())
        }
    }

    #[async_trait]
    impl MemoryStore for MockStore {
        async fn list_thread_ids_with_memory(
            &self,
        ) -> Result<Vec<Uuid>, crate::traits::StateError> {
            let storage = self.storage.read().await;
            Ok(storage.keys().cloned().collect())
        }
    }

    #[test]
    fn test_memory_system_creation() {
        let config = MemorySystemConfig::default();
        let store = MockStore::new();
        let _system = MemorySystem::new(config, store);
    }

    struct FailingLoadStore;

    #[async_trait]
    impl MemoryPersistence for FailingLoadStore {
        async fn load_memory_document(
            &self,
            _thread_id: Uuid,
        ) -> Result<MemoryDocument, crate::traits::StateError> {
            Err(StateError::Backend("memory backend unavailable".to_string()))
        }

        async fn save_memory_document(
            &self,
            _thread_id: Uuid,
            _doc: &MemoryDocument,
        ) -> Result<(), crate::traits::StateError> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn add_fact_preserves_state_source_context_and_category() {
        let system = MemorySystem::new(MemorySystemConfig::default(), FailingLoadStore);
        let error = system
            .add_fact(
                Uuid::new_v4(),
                "prefers terminal editors".to_string(),
                FactCategory::Preference,
                0.9,
            )
            .await
            .expect_err("load should fail");

        assert_eq!(error.category(), StateErrorCategory::ExternalConnection);

        match error {
            MemorySystemError::StateOperation { operation, source } => {
                assert_eq!(operation, "load memory for manual fact insertion");
                match source {
                    StateError::Backend(message) => {
                        assert!(message.contains("memory backend unavailable"));
                    }
                    other => panic!("unexpected source: {other}"),
                }
            }
            other => panic!("unexpected error: {other}"),
        }
    }

    #[tokio::test]
    async fn merge_thread_memories_maps_runtime_failures_to_runtime_category() {
        let system = MemorySystem::new(MemorySystemConfig::default(), MockStore::new());
        let error = system
            .merge_thread_memories(Box::new(MockLLMPort::new()), "user-1".to_string(), &[])
            .await
            .expect_err("empty thread memories should fail");

        assert_eq!(error.category(), StateErrorCategory::Runtime);

        match error {
            MemorySystemError::Runtime { operation, details } => {
                assert_eq!(operation, "merge thread memories");
                assert!(details.contains("No thread memories to merge"));
            }
            other => panic!("unexpected error: {other}"),
        }
    }
}
