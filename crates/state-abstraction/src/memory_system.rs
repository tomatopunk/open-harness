//! Unified memory system facade - integrates all components.

use agent_ports::ThreadId;
use chrono::Utc;
use std::collections::HashSet;
use uuid::Uuid;

use crate::memory_atomic::AtomicMemoryStore;
use crate::memory_document::{
    estimate_tokens, ArchivedContextSegment, ArchivedMemory, CompressionDecision, CompressionLevel,
    CompressionTriggerKind, Fact, FactCategory, MemoryDocument, RecentContextSegment,
    WorkingContextSegment, WorkingMemorySummary, MEMORY_DOCUMENT_SCHEMA_VERSION,
};
use crate::memory_extractor::FactExtractor;
use crate::memory_injection::{
    inject_memory_to_prompt, inject_memory_to_prompt_with_query, MemoryInjectionConfig,
};
use crate::memory_manager::FactManager;
use crate::memory_merge::{MemoryMergeEngine, UserProfile};
use crate::memory_retrieval::{
    build_sparse_vector, extract_semantic_terms, retrieve_segmented_context,
    SegmentedMemoryRetrieval, SimpleTokenCounter,
};
use crate::memory_voting::MemoryVotingEngine;
use crate::traits::MemoryPersistence;
use thiserror::Error;

/// Configuration for the memory system.
#[derive(Debug, Clone)]
pub struct MemorySystemConfig {
    pub max_facts: usize,
    pub fact_confidence_threshold: f32,
    pub max_injection_tokens: usize,
    pub compression_token_threshold: usize,
    pub milestone_snapshot_interval: usize,
    pub recent_fact_window: usize,
    pub working_fact_window: usize,
    pub archived_retrieval_limit: usize,
}

impl Default for MemorySystemConfig {
    fn default() -> Self {
        Self {
            max_facts: 100,
            fact_confidence_threshold: 0.7,
            max_injection_tokens: 2000,
            compression_token_threshold: 256,
            milestone_snapshot_interval: 6,
            recent_fact_window: 6,
            working_fact_window: 12,
            archived_retrieval_limit: 4,
        }
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

#[derive(Debug, Clone, Copy)]
struct CompressionRunTrigger {
    kind: CompressionTriggerKind,
    milestone_snapshot: Option<usize>,
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
        let mut doc = self.store.load_memory(thread_id).await?;
        self.synchronize_segmented_context(&mut doc, false);
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
        let mut prepared = doc.clone();
        self.synchronize_segmented_context(&mut prepared, true);
        self.store.save_memory_atomic(thread_id, &prepared).await
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
        inject_memory_to_prompt(
            system_prompt,
            memory,
            &self.injection_config(),
            &self.voting_engine,
        )
    }

    pub fn inject_to_prompt_with_query(
        &self,
        system_prompt: &str,
        memory: &MemoryDocument,
        query: &str,
    ) -> String {
        inject_memory_to_prompt_with_query(
            system_prompt,
            memory,
            &self.injection_config(),
            &self.voting_engine,
            Some(query),
        )
    }

    pub async fn retrieve_relevant_context(
        &self,
        thread_id: Uuid,
        query: &str,
    ) -> Result<SegmentedMemoryRetrieval, MemorySystemError> {
        let memory = self.load_memory(thread_id).await.map_err(|error| {
            MemorySystemError::state_operation("load memory for context retrieval", error)
        })?;
        let token_counter = SimpleTokenCounter::with_defaults();

        Ok(retrieve_segmented_context(
            &memory,
            Some(query),
            self.config.max_injection_tokens,
            &self.voting_engine,
            &token_counter,
            self.config.archived_retrieval_limit,
        ))
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

    fn injection_config(&self) -> MemoryInjectionConfig {
        MemoryInjectionConfig {
            max_tokens: self.config.max_injection_tokens,
            include_user_context: true,
            include_history: true,
            include_facts: true,
            facts_only: false,
            archived_retrieval_limit: self.config.archived_retrieval_limit,
        }
    }

    fn synchronize_segmented_context(&self, memory: &mut MemoryDocument, record_audit: bool) {
        memory.schema_version = MEMORY_DOCUMENT_SCHEMA_VERSION;
        let mut facts = memory.facts.clone();
        facts.sort_by(|left, right| right.created_at.cmp(&left.created_at));

        memory.segmented_context.recent = self.build_recent_segment(&facts);

        if let Some(trigger) = self.determine_compression_trigger(memory, &facts) {
            let recent_end = self.config.recent_fact_window.min(facts.len());
            let working_end = (recent_end + self.config.working_fact_window).min(facts.len());
            let (working_segment, working_decision) =
                self.build_working_segment(&facts[recent_end..working_end], trigger.kind);
            let (archived_segment, archived_decision) =
                self.build_archived_segment(&facts[working_end..], trigger.kind);

            memory.segmented_context.working = working_segment;
            memory.segmented_context.archived = archived_segment;

            if record_audit {
                if let Some(decision) = working_decision {
                    self.push_compression_decision(memory, decision);
                }

                if let Some(decision) = archived_decision {
                    self.push_compression_decision(memory, decision);
                }

                if let Some(snapshot) = trigger.milestone_snapshot {
                    memory.segmented_context.last_milestone_snapshot = snapshot;
                    memory.metadata.milestone_snapshots += 1;
                }
            }
        } else {
            memory.segmented_context.working = WorkingContextSegment::default();
            memory.segmented_context.archived = ArchivedContextSegment::default();
        }

        if record_audit {
            memory.metadata.last_updated = Some(Utc::now());
        }
    }

    fn determine_compression_trigger(
        &self,
        memory: &MemoryDocument,
        facts: &[Fact],
    ) -> Option<CompressionRunTrigger> {
        let token_count_before: usize =
            facts.iter().map(|fact| estimate_tokens(&fact.content)).sum();
        if self.config.compression_token_threshold > 0
            && token_count_before >= self.config.compression_token_threshold
        {
            return Some(CompressionRunTrigger {
                kind: CompressionTriggerKind::TokenThreshold,
                milestone_snapshot: None,
            });
        }

        if self.config.milestone_snapshot_interval == 0 || facts.is_empty() {
            return None;
        }

        let snapshot = facts.len() / self.config.milestone_snapshot_interval;
        if facts.len() % self.config.milestone_snapshot_interval == 0
            && snapshot > memory.segmented_context.last_milestone_snapshot
        {
            return Some(CompressionRunTrigger {
                kind: CompressionTriggerKind::MilestoneSnapshot,
                milestone_snapshot: Some(snapshot),
            });
        }

        None
    }

    fn build_recent_segment(&self, facts: &[Fact]) -> RecentContextSegment {
        let selected: Vec<_> =
            facts.iter().take(self.config.recent_fact_window.max(1)).cloned().collect();
        let estimated_tokens = selected.iter().map(|fact| estimate_tokens(&fact.content)).sum();

        RecentContextSegment { facts: selected, estimated_tokens, updated_at: Some(Utc::now()) }
    }

    fn build_working_segment(
        &self,
        facts: &[Fact],
        trigger: CompressionTriggerKind,
    ) -> (WorkingContextSegment, Option<CompressionDecision>) {
        if facts.is_empty() {
            return (WorkingContextSegment::default(), None);
        }

        let chunk_size = self.config.recent_fact_window.clamp(2, 4);
        let summaries: Vec<_> = facts
            .chunks(chunk_size)
            .map(|chunk| {
                let summary = self.summarize_facts(chunk);
                let estimated_tokens = estimate_tokens(&summary);
                WorkingMemorySummary::new(summary, chunk, trigger, estimated_tokens)
            })
            .collect();

        let token_count_before = facts.iter().map(|fact| estimate_tokens(&fact.content)).sum();
        let token_count_after = summaries.iter().map(|summary| summary.estimated_tokens).sum();
        let decision = CompressionDecision::new(
            CompressionLevel::Summary,
            trigger,
            facts.iter().map(|fact| fact.id.clone()).collect(),
            summaries.iter().map(|summary| summary.id.clone()).collect(),
            facts.iter().filter(|fact| fact.is_mandatory()).map(|fact| fact.id.clone()).collect(),
            self.select_key_facts(facts, 3).into_iter().map(|fact| fact.id).collect(),
            token_count_before,
            token_count_after,
            format!(
                "summary-compressed {} facts into {} working summaries",
                facts.len(),
                summaries.len()
            ),
        );

        (
            WorkingContextSegment {
                estimated_tokens: token_count_after,
                summaries,
                updated_at: Some(Utc::now()),
            },
            Some(decision),
        )
    }

    fn build_archived_segment(
        &self,
        facts: &[Fact],
        trigger: CompressionTriggerKind,
    ) -> (ArchivedContextSegment, Option<CompressionDecision>) {
        if facts.is_empty() {
            return (ArchivedContextSegment::default(), None);
        }

        let entries: Vec<_> = self
            .cluster_archived_facts(facts)
            .into_iter()
            .map(|cluster| {
                let key_facts = self.select_key_facts(&cluster, 3);
                let summary = self.summarize_facts(&cluster);
                let semantic_terms = extract_semantic_terms(
                    &cluster.iter().map(|fact| fact.content.as_str()).collect::<Vec<_>>().join(" "),
                );
                let vector = build_sparse_vector(&format!(
                    "{} {}",
                    summary,
                    key_facts
                        .iter()
                        .map(|fact| fact.content.as_str())
                        .collect::<Vec<_>>()
                        .join(" ")
                ));
                let estimated_tokens = estimate_tokens(&summary)
                    + key_facts.iter().map(|fact| estimate_tokens(&fact.content)).sum::<usize>();

                ArchivedMemory::new(
                    summary,
                    key_facts,
                    &cluster,
                    semantic_terms,
                    vector,
                    trigger,
                    estimated_tokens,
                )
            })
            .collect();

        let token_count_before = facts.iter().map(|fact| estimate_tokens(&fact.content)).sum();
        let token_count_after = entries.iter().map(|entry| entry.estimated_tokens).sum();
        let decision = CompressionDecision::new(
            CompressionLevel::Semantic,
            trigger,
            facts.iter().map(|fact| fact.id.clone()).collect(),
            entries.iter().map(|entry| entry.id.clone()).collect(),
            facts.iter().filter(|fact| fact.is_mandatory()).map(|fact| fact.id.clone()).collect(),
            entries
                .iter()
                .flat_map(|entry| entry.key_facts.iter().map(|fact| fact.id.clone()))
                .collect(),
            token_count_before,
            token_count_after,
            format!(
                "semantically compressed {} facts into {} archived entries",
                facts.len(),
                entries.len()
            ),
        );

        (
            ArchivedContextSegment {
                estimated_tokens: token_count_after,
                entries,
                updated_at: Some(Utc::now()),
            },
            Some(decision),
        )
    }

    fn push_compression_decision(
        &self,
        memory: &mut MemoryDocument,
        decision: CompressionDecision,
    ) {
        let duplicate = memory
            .segmented_context
            .compression_log
            .last()
            .map(|last| {
                last.level == decision.level
                    && last.trigger == decision.trigger
                    && last.source_fact_ids == decision.source_fact_ids
                    && last.output_entry_ids == decision.output_entry_ids
            })
            .unwrap_or(false);

        if !duplicate {
            memory.segmented_context.compression_log.push(decision);
            memory.metadata.compression_count += 1;
        }
    }

    fn summarize_facts(&self, facts: &[Fact]) -> String {
        self.select_key_facts(facts, 3)
            .into_iter()
            .map(|fact| format!("[{}] {}", fact.category.as_str(), fact.content))
            .collect::<Vec<_>>()
            .join("; ")
    }

    fn select_key_facts(&self, facts: &[Fact], limit: usize) -> Vec<Fact> {
        let mut ranked = facts.to_vec();
        ranked.sort_by(|left, right| {
            right
                .is_mandatory()
                .cmp(&left.is_mandatory())
                .then_with(|| {
                    right
                        .confidence
                        .partial_cmp(&left.confidence)
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
                .then_with(|| right.created_at.cmp(&left.created_at))
        });

        let mandatory_count = ranked.iter().filter(|fact| fact.is_mandatory()).count();
        let target = limit.max(mandatory_count);
        let mut selected = Vec::new();

        for fact in ranked {
            if selected.iter().any(|existing: &Fact| existing.id == fact.id) {
                continue;
            }

            selected.push(fact);
            if selected.len() >= target {
                break;
            }
        }

        selected
    }

    fn cluster_archived_facts(&self, facts: &[Fact]) -> Vec<Vec<Fact>> {
        let mut clusters: Vec<(HashSet<String>, Vec<Fact>)> = Vec::new();

        for fact in facts {
            let fact_terms: HashSet<String> =
                extract_semantic_terms(&fact.content).into_iter().collect();
            if let Some((cluster_terms, cluster_facts)) =
                clusters.iter_mut().find(|(terms, items)| {
                    !fact_terms.is_empty() && !terms.is_disjoint(&fact_terms)
                        || items
                            .first()
                            .map(|existing| existing.category == fact.category)
                            .unwrap_or(false)
                })
            {
                cluster_terms.extend(fact_terms.clone());
                cluster_facts.push(fact.clone());
            } else {
                clusters.push((fact_terms, vec![fact.clone()]));
            }
        }

        clusters.into_iter().map(|(_, facts)| facts).collect()
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

    #[tokio::test]
    async fn save_memory_applies_token_threshold_compression() {
        let config = MemorySystemConfig {
            compression_token_threshold: 12,
            milestone_snapshot_interval: 100,
            recent_fact_window: 1,
            working_fact_window: 2,
            ..MemorySystemConfig::default()
        };
        let store = MockStore::new();
        let system = MemorySystem::new(config, store);
        let thread_id = Uuid::new_v4();
        let mut doc = MemoryDocument::default();

        for entry in [
            "Project Atlas uses PostgreSQL for analytics workloads",
            "The release train shares migrations through the state abstraction layer",
            "Operators prefer milestone snapshots for long sessions",
            "The assistant should preserve deployment constraints in memory",
        ] {
            doc.add_fact(Fact::new(
                entry.to_string(),
                FactCategory::Knowledge,
                0.9,
                thread_id.to_string(),
            ));
        }

        system.save_memory(thread_id, &doc).await.unwrap();
        let loaded = system.load_memory(thread_id).await.unwrap();

        assert!(!loaded.segmented_context.working.summaries.is_empty());
        assert!(loaded
            .segmented_context
            .compression_log
            .iter()
            .any(|decision| decision.trigger == CompressionTriggerKind::TokenThreshold));
    }

    #[tokio::test]
    async fn save_memory_applies_milestone_snapshot_compression() {
        let config = MemorySystemConfig {
            compression_token_threshold: usize::MAX,
            milestone_snapshot_interval: 3,
            recent_fact_window: 1,
            working_fact_window: 1,
            ..MemorySystemConfig::default()
        };
        let store = MockStore::new();
        let system = MemorySystem::new(config, store);
        let thread_id = Uuid::new_v4();
        let mut doc = MemoryDocument::default();

        for entry in [
            "The user works on long-running refactors",
            "Milestone snapshots should remain auditable",
            "Recent context should stay uncompressed",
        ] {
            doc.add_fact(Fact::new(
                entry.to_string(),
                FactCategory::Context,
                0.8,
                thread_id.to_string(),
            ));
        }

        system.save_memory(thread_id, &doc).await.unwrap();
        let loaded = system.load_memory(thread_id).await.unwrap();

        assert_eq!(loaded.segmented_context.last_milestone_snapshot, 1);
        assert!(loaded
            .segmented_context
            .compression_log
            .iter()
            .any(|decision| decision.trigger == CompressionTriggerKind::MilestoneSnapshot));
    }

    #[tokio::test]
    async fn retrieve_relevant_context_recovers_archived_key_fact_after_compression() {
        let config = MemorySystemConfig {
            compression_token_threshold: 10,
            milestone_snapshot_interval: 100,
            recent_fact_window: 1,
            working_fact_window: 1,
            ..MemorySystemConfig::default()
        };
        let store = MockStore::new();
        let system = MemorySystem::new(config, store);
        let thread_id = Uuid::new_v4();
        let mut doc = MemoryDocument::default();

        doc.add_fact(Fact::new(
            "Project Atlas uses PostgreSQL for analytics storage".to_string(),
            FactCategory::Knowledge,
            0.95,
            thread_id.to_string(),
        ));

        for entry in [
            "Recent work focuses on runtime sandbox auditing",
            "The team is tightening session core invariants",
            "Compression should preserve key retrieval paths",
            "Vector-first memory should prefer semantic matches",
        ] {
            doc.add_fact(Fact::new(
                entry.to_string(),
                FactCategory::Knowledge,
                0.7,
                thread_id.to_string(),
            ));
        }

        system.save_memory(thread_id, &doc).await.unwrap();
        let retrieval = system
            .retrieve_relevant_context(thread_id, "Which database does Project Atlas use?")
            .await
            .unwrap();

        assert!(!retrieval.archived_matches.is_empty());
        assert!(retrieval.archived_matches.iter().any(|matched| {
            matched.entry.key_facts.iter().any(|fact| fact.content.contains("PostgreSQL"))
        }));
    }

    #[tokio::test]
    async fn compression_preserves_mandatory_fact_retrievability() {
        let config = MemorySystemConfig {
            compression_token_threshold: 10,
            milestone_snapshot_interval: 100,
            recent_fact_window: 1,
            working_fact_window: 1,
            ..MemorySystemConfig::default()
        };
        let store = MockStore::new();
        let system = MemorySystem::new(config, store);
        let thread_id = Uuid::new_v4();
        let mut doc = MemoryDocument::default();

        let mandatory_fact = Fact::new(
            "Production deploy window is Friday 18:00 UTC".to_string(),
            FactCategory::Goal,
            0.92,
            thread_id.to_string(),
        )
        .with_mandatory(true);
        let mandatory_fact_id = mandatory_fact.id.clone();
        doc.add_fact(mandatory_fact);

        for entry in [
            "Filler fact about plugin discovery order",
            "Filler fact about event bus observability",
            "Filler fact about MCP reconnect safety",
            "Filler fact about sandbox execution tracing",
            "Filler fact about runtime state transitions",
        ] {
            doc.add_fact(Fact::new(
                entry.to_string(),
                FactCategory::Knowledge,
                0.5,
                thread_id.to_string(),
            ));
        }

        system.save_memory(thread_id, &doc).await.unwrap();
        let loaded = system.load_memory(thread_id).await.unwrap();

        assert!(loaded.segmented_context.compression_log.iter().any(|decision| {
            decision.mandatory_fact_ids.contains(&mandatory_fact_id)
                && decision.preserved_fact_ids.contains(&mandatory_fact_id)
        }));

        let retrieval = system
            .retrieve_relevant_context(thread_id, "when is the production deploy window")
            .await
            .unwrap();

        assert!(retrieval.archived_matches.iter().any(|matched| {
            matched.entry.key_facts.iter().any(|fact| fact.id == mandatory_fact_id)
        }));
    }
}
