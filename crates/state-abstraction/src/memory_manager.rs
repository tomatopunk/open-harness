//! Fact manager for deduplication, conflict resolution, and LRU eviction.

use crate::memory_document::{Fact, MemoryDocument};
use crate::memory_voting::MemoryVotingEngine;
use chrono::Utc;
use std::collections::HashMap;

/// Statistics about fact management operations.
#[derive(Debug, Clone, Default)]
pub struct FactStats {
    pub total_facts: usize,
    pub duplicates_removed: usize,
    pub conflicts_resolved: usize,
    pub facts_evicted: usize,
}

/// Manager for fact lifecycle operations.
pub struct FactManager {
    voting_engine: MemoryVotingEngine,
    max_facts: usize,
    enable_dedup: bool,
    enable_conflict_resolution: bool,
    enable_eviction: bool,
}

impl FactManager {
    /// Create a new fact manager.
    pub fn new(voting_engine: MemoryVotingEngine, max_facts: usize) -> Self {
        Self {
            voting_engine,
            max_facts,
            enable_dedup: true,
            enable_conflict_resolution: true,
            enable_eviction: true,
        }
    }

    /// Create with defaults.
    pub fn with_defaults() -> Self {
        Self::new(MemoryVotingEngine::with_defaults(), 100)
    }

    /// Enable or disable deduplication.
    pub fn set_dedup(&mut self, enabled: bool) {
        self.enable_dedup = enabled;
    }

    /// Enable or disable conflict resolution.
    pub fn set_conflict_resolution(&mut self, enabled: bool) {
        self.enable_conflict_resolution = enabled;
    }

    /// Enable or disable eviction.
    pub fn set_eviction(&mut self, enabled: bool) {
        self.enable_eviction = enabled;
    }

    /// Process and manage a collection of facts.
    pub fn process(&self, facts: Vec<Fact>) -> (Vec<Fact>, FactStats) {
        let mut stats = FactStats { total_facts: facts.len(), ..Default::default() };

        let mut processed = facts;

        // Step 1: Deduplication
        if self.enable_dedup {
            processed = self.deduplicate(processed, &mut stats);
        }

        // Step 2: Conflict resolution
        if self.enable_conflict_resolution {
            processed = self.resolve_conflicts(processed, &mut stats);
        }

        // Step 3: Eviction if over limit
        if self.enable_eviction && processed.len() > self.max_facts {
            processed = self.evict(processed, &mut stats);
        }

        (processed, stats)
    }

    /// Apply management to a MemoryDocument.
    pub fn apply_to_document(&self, doc: &mut MemoryDocument) -> FactStats {
        let facts = std::mem::take(&mut doc.facts);
        let (processed, stats) = self.process(facts);
        doc.facts = processed;
        doc.metadata.total_facts_processed += stats.total_facts as u64;
        doc.metadata.facts_removed +=
            (stats.duplicates_removed + stats.conflicts_resolved + stats.facts_evicted) as u64;
        doc.metadata.last_updated = Some(Utc::now());
        stats
    }

    /// Remove duplicate facts based on content hash.
    fn deduplicate(&self, facts: Vec<Fact>, stats: &mut FactStats) -> Vec<Fact> {
        let mut seen: HashMap<String, Fact> = HashMap::new();

        for fact in facts {
            let key = self.fact_key(&fact.content);

            seen.entry(key)
                .and_modify(|existing| {
                    // Keep the one with higher confidence
                    if fact.confidence > existing.confidence {
                        *existing = fact.clone();
                    }
                })
                .or_insert(fact);
        }

        let result: Vec<Fact> = seen.into_values().collect();
        stats.duplicates_removed = stats.total_facts - result.len();
        result
    }

    /// Resolve conflicts between facts.
    fn resolve_conflicts(&self, facts: Vec<Fact>, stats: &mut FactStats) -> Vec<Fact> {
        if facts.is_empty() {
            return facts;
        }

        let before_count = facts.len();
        let result = self.voting_engine.resolve_conflict(&facts);
        let resolved: Vec<Fact> = result.all_candidates.into_iter().map(|v| v.fact).collect();
        stats.conflicts_resolved = before_count - resolved.len();

        resolved
    }

    /// Evict low-priority facts when over limit.
    fn evict(&self, mut facts: Vec<Fact>, stats: &mut FactStats) -> Vec<Fact> {
        if facts.len() <= self.max_facts {
            return facts;
        }

        // Rank facts by weight
        let ranked = self.voting_engine.rank_facts(&facts);

        // Keep top max_facts
        facts.clear();
        for ranked_fact in ranked.into_iter().take(self.max_facts) {
            facts.push(ranked_fact.fact);
        }

        stats.facts_evicted = facts.len() - self.max_facts;
        facts
    }

    /// Generate a key for fact deduplication.
    fn fact_key(&self, content: &str) -> String {
        // Simple normalization: lowercase and trim
        content.trim().to_lowercase()
    }

    /// Merge new facts into existing facts.
    pub fn merge_facts(&self, existing: Vec<Fact>, new: Vec<Fact>) -> (Vec<Fact>, FactStats) {
        let mut all_facts = existing;
        all_facts.extend(new);
        self.process(all_facts)
    }
}

impl Default for FactManager {
    fn default() -> Self {
        Self::with_defaults()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory_document::FactCategory;

    #[test]
    fn test_deduplication() {
        let manager = FactManager::with_defaults();
        let facts = vec![
            Fact::new("Test".to_string(), FactCategory::Knowledge, 0.5, "t1".to_string()),
            Fact::new("Test".to_string(), FactCategory::Knowledge, 0.9, "t2".to_string()), // Duplicate with higher confidence
        ];

        let (processed, stats) = manager.process(facts);
        assert_eq!(processed.len(), 1);
        assert_eq!(stats.duplicates_removed, 1);
        assert_eq!(processed[0].confidence, 0.9); // Kept higher confidence
    }

    #[test]
    fn test_eviction() {
        let mut manager = FactManager::with_defaults();
        manager.max_facts = 3;

        let facts = vec![
            Fact::new("Fact 1".to_string(), FactCategory::Knowledge, 0.3, "t1".to_string()),
            Fact::new("Fact 2".to_string(), FactCategory::Knowledge, 0.9, "t2".to_string()),
            Fact::new("Fact 3".to_string(), FactCategory::Knowledge, 0.5, "t3".to_string()),
            Fact::new("Fact 4".to_string(), FactCategory::Knowledge, 0.7, "t4".to_string()),
            Fact::new("Fact 5".to_string(), FactCategory::Knowledge, 0.1, "t5".to_string()),
        ];

        let (processed, _stats) = manager.process(facts);
        assert_eq!(processed.len(), 3);
        // Eviction may or may not happen depending on voting/conflict resolution
        // Just verify we have the right number of facts

        // Should keep highest confidence facts
        let confidences: Vec<f32> = processed.iter().map(|f| f.confidence).collect();
        assert!(confidences.iter().all(|&c| c >= 0.5));
    }

    #[test]
    fn test_merge_facts() {
        let manager = FactManager::with_defaults();

        let existing = vec![Fact::new(
            "Existing 1".to_string(),
            FactCategory::Knowledge,
            0.8,
            "t1".to_string(),
        )];

        let new =
            vec![Fact::new("New 1".to_string(), FactCategory::Knowledge, 0.9, "t2".to_string())];

        let (merged, _stats) = manager.merge_facts(existing, new);
        assert_eq!(merged.len(), 2);
    }

    #[test]
    fn test_apply_to_document() {
        let manager = FactManager::with_defaults();
        let mut doc = MemoryDocument::default();

        doc.add_fact(Fact::new(
            "Fact 1".to_string(),
            FactCategory::Knowledge,
            0.5,
            "t1".to_string(),
        ));
        doc.add_fact(Fact::new(
            "Fact 1".to_string(),
            FactCategory::Knowledge,
            0.9,
            "t2".to_string(),
        )); // Duplicate

        let stats = manager.apply_to_document(&mut doc);

        assert_eq!(doc.facts.len(), 1);
        assert!(stats.duplicates_removed > 0);
        assert!(doc.metadata.last_updated.is_some());
    }
}
