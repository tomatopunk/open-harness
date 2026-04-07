//! Structured memory payload (facts + optional user/history JSON), versioned for evolution without binding vector DBs into core.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Bump when adding/removing top-level fields in [`MemoryDocument`].
pub const MEMORY_DOCUMENT_SCHEMA_VERSION: u32 = 3;

/// Fact categories for long-term memory classification (DeerFlow-inspired).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum FactCategory {
    /// User preferences (tools, styles, methods).
    Preference,
    /// Professional knowledge and technical domains.
    #[default]
    Knowledge,
    /// Background information (work, projects, location).
    Context,
    /// Behavioral patterns and work habits.
    Behavior,
    /// Goals and plans.
    Goal,
}

impl FactCategory {
    /// Get the string representation of the category.
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Preference => "preference",
            Self::Knowledge => "knowledge",
            Self::Context => "context",
            Self::Behavior => "behavior",
            Self::Goal => "goal",
        }
    }
}

/// A structured fact with metadata for long-term memory.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Fact {
    /// Unique identifier for the fact.
    pub id: String,
    /// The fact content.
    pub content: String,
    /// Category of the fact.
    pub category: FactCategory,
    /// Confidence score (0.0 - 1.0).
    pub confidence: f32,
    /// When the fact was created.
    pub created_at: DateTime<Utc>,
    /// Source thread ID where the fact was extracted.
    pub source_thread: String,
    /// Optional: when the fact was last updated.
    #[serde(default)]
    pub updated_at: Option<DateTime<Utc>>,
    /// Optional: weight for voting (computed as confidence * recency * source_reliability).
    #[serde(default)]
    pub weight: Option<f32>,
    #[serde(default)]
    pub mandatory: bool,
}

impl Fact {
    /// Create a new fact with auto-generated ID and timestamp.
    pub fn new(
        content: String,
        category: FactCategory,
        confidence: f32,
        source_thread: String,
    ) -> Self {
        let now = Utc::now();
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            content,
            category,
            confidence,
            created_at: now,
            source_thread,
            updated_at: None,
            weight: None,
            mandatory: false,
        }
    }

    /// Compute the voting weight for this fact.
    /// Formula: weight = confidence * recency_factor * source_reliability
    #[must_use]
    pub fn compute_weight(&self, half_life_days: f32, source_reliability: f32) -> f32 {
        let days_since_created = (Utc::now() - self.created_at).num_seconds() as f32 / 86400.0;
        let recency_factor = (-days_since_created / half_life_days).exp();
        self.confidence * recency_factor * source_reliability
    }

    /// Update the fact content and timestamp.
    pub fn update(&mut self, new_content: String, new_confidence: f32) {
        self.content = new_content;
        self.confidence = new_confidence;
        self.updated_at = Some(Utc::now());
    }

    #[must_use]
    pub fn with_mandatory(mut self, mandatory: bool) -> Self {
        self.mandatory = mandatory;
        self
    }

    #[must_use]
    pub fn is_mandatory(&self) -> bool {
        self.mandatory || matches!(self.category, FactCategory::Goal) || self.confidence >= 0.95
    }

    /// Get the string representation of the category.
    #[must_use]
    pub fn category_str(&self) -> &'static str {
        self.category.as_str()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompressionLevel {
    Summary,
    Semantic,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompressionTriggerKind {
    TokenThreshold,
    MilestoneSnapshot,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SparseVector {
    #[serde(default)]
    pub dimensions: Vec<String>,
    #[serde(default)]
    pub values: Vec<f32>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RecentContextSegment {
    #[serde(default)]
    pub facts: Vec<Fact>,
    #[serde(default)]
    pub estimated_tokens: usize,
    #[serde(default)]
    pub updated_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkingMemorySummary {
    pub id: String,
    pub summary: String,
    #[serde(default)]
    pub source_fact_ids: Vec<String>,
    #[serde(default)]
    pub mandatory_fact_ids: Vec<String>,
    pub trigger: CompressionTriggerKind,
    #[serde(default)]
    pub estimated_tokens: usize,
    pub created_at: DateTime<Utc>,
}

impl WorkingMemorySummary {
    #[must_use]
    pub fn new(
        summary: String,
        source_facts: &[Fact],
        trigger: CompressionTriggerKind,
        estimated_tokens: usize,
    ) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            summary,
            source_fact_ids: source_facts.iter().map(|fact| fact.id.clone()).collect(),
            mandatory_fact_ids: source_facts
                .iter()
                .filter(|fact| fact.is_mandatory())
                .map(|fact| fact.id.clone())
                .collect(),
            trigger,
            estimated_tokens,
            created_at: Utc::now(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct WorkingContextSegment {
    #[serde(default)]
    pub summaries: Vec<WorkingMemorySummary>,
    #[serde(default)]
    pub estimated_tokens: usize,
    #[serde(default)]
    pub updated_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ArchivedMemory {
    pub id: String,
    pub summary: String,
    #[serde(default)]
    pub key_facts: Vec<Fact>,
    #[serde(default)]
    pub source_fact_ids: Vec<String>,
    #[serde(default)]
    pub mandatory_fact_ids: Vec<String>,
    #[serde(default)]
    pub semantic_terms: Vec<String>,
    #[serde(default)]
    pub vector: SparseVector,
    pub trigger: CompressionTriggerKind,
    #[serde(default)]
    pub estimated_tokens: usize,
    pub created_at: DateTime<Utc>,
}

impl ArchivedMemory {
    #[must_use]
    pub fn new(
        summary: String,
        key_facts: Vec<Fact>,
        source_facts: &[Fact],
        semantic_terms: Vec<String>,
        vector: SparseVector,
        trigger: CompressionTriggerKind,
        estimated_tokens: usize,
    ) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            summary,
            key_facts,
            source_fact_ids: source_facts.iter().map(|fact| fact.id.clone()).collect(),
            mandatory_fact_ids: source_facts
                .iter()
                .filter(|fact| fact.is_mandatory())
                .map(|fact| fact.id.clone())
                .collect(),
            semantic_terms,
            vector,
            trigger,
            estimated_tokens,
            created_at: Utc::now(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ArchivedContextSegment {
    #[serde(default)]
    pub entries: Vec<ArchivedMemory>,
    #[serde(default)]
    pub estimated_tokens: usize,
    #[serde(default)]
    pub updated_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CompressionDecision {
    pub id: String,
    pub level: CompressionLevel,
    pub trigger: CompressionTriggerKind,
    #[serde(default)]
    pub source_fact_ids: Vec<String>,
    #[serde(default)]
    pub output_entry_ids: Vec<String>,
    #[serde(default)]
    pub mandatory_fact_ids: Vec<String>,
    #[serde(default)]
    pub preserved_fact_ids: Vec<String>,
    #[serde(default)]
    pub token_count_before: usize,
    #[serde(default)]
    pub token_count_after: usize,
    pub reason: String,
    pub created_at: DateTime<Utc>,
}

impl CompressionDecision {
    #[must_use]
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        level: CompressionLevel,
        trigger: CompressionTriggerKind,
        source_fact_ids: Vec<String>,
        output_entry_ids: Vec<String>,
        mandatory_fact_ids: Vec<String>,
        preserved_fact_ids: Vec<String>,
        token_count_before: usize,
        token_count_after: usize,
        reason: impl Into<String>,
    ) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            level,
            trigger,
            source_fact_ids,
            output_entry_ids,
            mandatory_fact_ids,
            preserved_fact_ids,
            token_count_before,
            token_count_after,
            reason: reason.into(),
            created_at: Utc::now(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SegmentedContext {
    #[serde(default)]
    pub recent: RecentContextSegment,
    #[serde(default)]
    pub working: WorkingContextSegment,
    #[serde(default)]
    pub archived: ArchivedContextSegment,
    #[serde(default)]
    pub compression_log: Vec<CompressionDecision>,
    #[serde(default)]
    pub last_milestone_snapshot: usize,
}

impl SegmentedContext {
    #[must_use]
    pub fn has_any_content(&self) -> bool {
        !self.recent.facts.is_empty()
            || !self.working.summaries.is_empty()
            || !self.archived.entries.is_empty()
    }
}

/// User profile context (work/personal/top-of-mind).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct MemoryUserProfile {
    /// Work context summary (2-3 sentences).
    #[serde(default)]
    pub work_context: Option<String>,
    /// Personal context summary (1-2 sentences).
    #[serde(default)]
    pub personal_context: Option<String>,
    /// Current top-of-mind topics (3-5 sentences).
    #[serde(default)]
    pub top_of_mind: Option<String>,
    /// Last update timestamp.
    #[serde(default)]
    pub updated_at: Option<DateTime<Utc>>,
}

/// Historical memory context (time-based).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct MemoryHistory {
    /// Recent months summary (1-3 months, 4-6 sentences).
    #[serde(default)]
    pub recent_months: Option<String>,
    /// Earlier context (3-12 months, 3-5 sentences).
    #[serde(default)]
    pub earlier_context: Option<String>,
    /// Long-term background (2-4 sentences).
    #[serde(default)]
    pub long_term_background: Option<String>,
    /// Last update timestamp.
    #[serde(default)]
    pub updated_at: Option<DateTime<Utc>>,
}

/// Memory metadata for versioning and auditing.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct MemoryMetadata {
    /// When the memory was last updated.
    #[serde(default)]
    pub last_updated: Option<DateTime<Utc>>,
    /// Total number of facts (including archived).
    #[serde(default)]
    pub total_facts_processed: u64,
    /// Number of facts removed due to conflicts or pruning.
    #[serde(default)]
    pub facts_removed: u64,
    /// Number of merge operations performed.
    #[serde(default)]
    pub merge_count: u64,
    #[serde(default)]
    pub compression_count: u64,
    #[serde(default)]
    pub milestone_snapshots: u64,
}

/// structured memory: facts list plus optional JSON blobs for user profile and history.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MemoryDocument {
    pub schema_version: u32,
    #[serde(default)]
    pub facts: Vec<Fact>,
    #[serde(default)]
    pub user: MemoryUserProfile,
    #[serde(default)]
    pub history: MemoryHistory,
    #[serde(default)]
    pub segmented_context: SegmentedContext,
    #[serde(default)]
    pub metadata: MemoryMetadata,
}

impl Default for MemoryDocument {
    fn default() -> Self {
        Self {
            schema_version: MEMORY_DOCUMENT_SCHEMA_VERSION,
            facts: Vec::new(),
            user: MemoryUserProfile::default(),
            history: MemoryHistory::default(),
            segmented_context: SegmentedContext::default(),
            metadata: MemoryMetadata::default(),
        }
    }
}

impl MemoryDocument {
    /// Whether this document should be listed as "having memory" for admin APIs.
    #[must_use]
    pub fn has_any_content(&self) -> bool {
        !self.facts.is_empty()
            || self.user.work_context.is_some()
            || self.user.personal_context.is_some()
            || self.user.top_of_mind.is_some()
            || self.history.recent_months.is_some()
            || self.history.earlier_context.is_some()
            || self.history.long_term_background.is_some()
            || self.segmented_context.has_any_content()
    }

    /// Add a fact to the document.
    pub fn add_fact(&mut self, fact: Fact) {
        self.facts.push(fact);
        self.metadata.total_facts_processed += 1;
        self.metadata.last_updated = Some(Utc::now());
    }

    /// Remove facts by ID.
    pub fn remove_facts(&mut self, fact_ids: &[String]) {
        let before = self.facts.len();
        self.facts.retain(|f| !fact_ids.contains(&f.id));
        self.metadata.facts_removed += (before - self.facts.len()) as u64;
        self.metadata.last_updated = Some(Utc::now());
    }

    /// Get facts by category.
    #[must_use]
    pub fn facts_by_category(&self, category: &FactCategory) -> Vec<&Fact> {
        self.facts.iter().filter(|f| &f.category == category).collect()
    }

    /// Get top N facts by weight (for injection into prompts).
    #[must_use]
    pub fn top_facts(&self, n: usize) -> Vec<&Fact> {
        let mut sorted: Vec<&Fact> = self.facts.iter().collect();
        sorted.sort_by(|a, b| {
            let weight_a = a.weight.unwrap_or(a.confidence);
            let weight_b = b.weight.unwrap_or(b.confidence);
            weight_b.partial_cmp(&weight_a).unwrap_or(std::cmp::Ordering::Equal)
        });
        sorted.into_iter().take(n).collect()
    }

    /// Compute weights for all facts.
    pub fn compute_all_weights(&mut self, half_life_days: f32, source_reliability: f32) {
        for fact in &mut self.facts {
            fact.weight = Some(fact.compute_weight(half_life_days, source_reliability));
        }
    }

    #[must_use]
    pub fn mandatory_facts(&self) -> Vec<&Fact> {
        self.facts.iter().filter(|fact| fact.is_mandatory()).collect()
    }

    #[must_use]
    pub fn estimated_fact_tokens(&self) -> usize {
        self.facts.iter().map(|fact| estimate_tokens(&fact.content)).sum()
    }

    /// Prune facts to max_count, keeping highest weight facts.
    pub fn prune_to_max(&mut self, max_count: usize, half_life_days: f32, source_reliability: f32) {
        if self.facts.len() <= max_count {
            return;
        }

        // Compute weights
        self.compute_all_weights(half_life_days, source_reliability);

        // Sort by weight descending
        self.facts.sort_by(|a, b| {
            let weight_a = a.weight.unwrap_or(a.confidence);
            let weight_b = b.weight.unwrap_or(b.confidence);
            weight_b.partial_cmp(&weight_a).unwrap_or(std::cmp::Ordering::Equal)
        });

        // Keep top max_count
        let removed = self.facts.split_off(max_count);
        self.metadata.facts_removed += removed.len() as u64;
        self.metadata.last_updated = Some(Utc::now());
    }
}

#[must_use]
pub fn estimate_tokens(text: &str) -> usize {
    let chars = text.trim().chars().count();
    if chars == 0 {
        return 0;
    }

    std::cmp::max(1, chars.div_ceil(4))
}

pub fn decode_memory_json_str(raw: &str) -> Result<MemoryDocument, String> {
    let t = raw.trim();
    if t.is_empty() {
        return Ok(MemoryDocument::default());
    }

    if let Ok(doc) = serde_json::from_str::<MemoryDocument>(t) {
        return Ok(doc);
    }

    Err("memory json: unable to decode".to_string())
}

/// Decode from a JSON value (Postgres JSON column).
pub fn decode_memory_json_value(v: &Value) -> Result<MemoryDocument, String> {
    if v.is_null() {
        return Ok(MemoryDocument::default());
    }
    decode_memory_json_str(&serde_json::to_string(v).map_err(|e| e.to_string())?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fact_creation() {
        let fact = Fact::new(
            "用户喜欢 Python".to_string(),
            FactCategory::Preference,
            0.95,
            "thread-123".to_string(),
        );

        assert_eq!(fact.content, "用户喜欢 Python");
        assert_eq!(fact.category, FactCategory::Preference);
        assert_eq!(fact.confidence, 0.95);
        assert!(!fact.id.is_empty());
    }

    #[test]
    fn test_fact_weight_computation() {
        let mut fact = Fact::new(
            "用户在北京".to_string(),
            FactCategory::Context,
            0.8,
            "thread-456".to_string(),
        );

        let weight = fact.compute_weight(30.0, 1.0);
        assert!(weight > 0.0);
        assert!(weight <= 0.8); // Should be <= confidence due to recency decay

        fact.weight = Some(weight);
        assert!(fact.weight.is_some());
    }

    #[test]
    fn test_memory_document_has_content() {
        let doc = MemoryDocument::default();
        assert!(!doc.has_any_content());

        let mut doc_with_facts = MemoryDocument::default();
        doc_with_facts.add_fact(Fact::new(
            "test".to_string(),
            FactCategory::Knowledge,
            0.5,
            "thread-1".to_string(),
        ));
        assert!(doc_with_facts.has_any_content());
    }

    #[test]
    fn test_memory_document_prune() {
        let mut doc = MemoryDocument::default();

        // Add 10 facts with different confidences
        for i in 0..10 {
            doc.add_fact(Fact::new(
                format!("fact {}", i),
                FactCategory::Knowledge,
                (i + 1) as f32 / 10.0,
                "thread-1".to_string(),
            ));
        }

        assert_eq!(doc.facts.len(), 10);

        // Prune to 5 facts
        doc.prune_to_max(5, 30.0, 1.0);

        assert_eq!(doc.facts.len(), 5);
        assert_eq!(doc.metadata.facts_removed, 5);

        // Verify highest confidence facts are kept
        for fact in &doc.facts {
            assert!(fact.confidence >= 0.6); // Should keep facts with confidence 0.6-1.0
        }
    }

    #[test]
    fn test_decode_legacy_json_fails() {
        let legacy_json = r#"["fact1", "fact2", "fact3"]"#;

        let error = decode_memory_json_str(legacy_json).unwrap_err();

        assert_eq!(error, "memory json: unable to decode");
    }

    #[test]
    fn test_decode_current_json() {
        let current_json = r#"{
            "schema_version": 2,
            "facts": [
                {
                    "id": "f1",
                    "content": "用户喜欢 Rust",
                    "category": "preference",
                    "confidence": 0.9,
                    "created_at": "2026-03-30T12:00:00Z",
                    "source_thread": "thread-1"
                }
            ],
            "user": {
                "work_context": "Software engineer"
            }
        }"#;

        let doc = decode_memory_json_str(current_json).unwrap();
        assert_eq!(doc.facts.len(), 1);
        assert_eq!(doc.facts[0].content, "用户喜欢 Rust");
        assert_eq!(doc.facts[0].category, FactCategory::Preference);
        assert_eq!(doc.user.work_context, Some("Software engineer".to_string()));
    }

    #[test]
    fn test_mandatory_fact_defaults_and_inference() {
        let explicit = Fact::new(
            "Do not rotate production secrets automatically".to_string(),
            FactCategory::Knowledge,
            0.8,
            "thread-1".to_string(),
        )
        .with_mandatory(true);
        assert!(explicit.is_mandatory());

        let goal = Fact::new(
            "Ship the memory refactor this week".to_string(),
            FactCategory::Goal,
            0.7,
            "thread-1".to_string(),
        );
        assert!(goal.is_mandatory());

        let routine = Fact::new(
            "Filler fact".to_string(),
            FactCategory::Knowledge,
            0.4,
            "thread-1".to_string(),
        );
        assert!(!routine.is_mandatory());
    }

    #[test]
    fn test_segmented_context_counts_as_memory_content() {
        let mut doc = MemoryDocument::default();
        doc.segmented_context.recent.facts.push(Fact::new(
            "Recent context".to_string(),
            FactCategory::Context,
            0.8,
            "thread-1".to_string(),
        ));

        assert!(doc.has_any_content());
    }
}
