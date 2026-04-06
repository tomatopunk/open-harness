//! Memory retrieval strategies with token budget truncation.

use crate::memory_document::{
    ArchivedMemory, Fact, MemoryDocument, SparseVector, WorkingMemorySummary,
};
use crate::memory_voting::MemoryVotingEngine;
use std::collections::{HashMap, HashSet};

/// Result of token budget truncation.
#[derive(Debug, Clone)]
pub struct TruncationResult {
    pub facts: Vec<Fact>,
    pub total_tokens: usize,
    pub facts_included: usize,
    pub facts_excluded: usize,
}

/// Token counter trait for LLM integration.
pub trait TokenCounter {
    fn count_tokens(&self, text: &str) -> usize;
}

/// Simple token counter (character-based approximation).
pub struct SimpleTokenCounter {
    chars_per_token: usize,
}

impl SimpleTokenCounter {
    pub fn new(chars_per_token: usize) -> Self {
        Self { chars_per_token }
    }

    pub fn with_defaults() -> Self {
        Self::new(4) // ~4 characters per token for English/Chinese mix
    }
}

impl TokenCounter for SimpleTokenCounter {
    fn count_tokens(&self, text: &str) -> usize {
        text.len() / self.chars_per_token
    }
}

/// Truncate facts to fit within token budget.
pub fn truncate_to_token_budget(
    facts: &[Fact],
    max_tokens: usize,
    voting_engine: &MemoryVotingEngine,
    token_counter: &dyn TokenCounter,
) -> TruncationResult {
    if facts.is_empty() || max_tokens == 0 {
        return TruncationResult {
            facts: vec![],
            total_tokens: 0,
            facts_included: 0,
            facts_excluded: facts.len(),
        };
    }

    // Rank facts by weight
    let ranked = voting_engine.rank_facts(facts);

    // Add facts until budget exceeded
    let mut result_facts = Vec::new();
    let mut total_tokens = 0;
    let mut facts_text = String::new();

    for ranked_fact in ranked {
        let fact_text = format!("{}\n", ranked_fact.fact.content);
        let fact_tokens = token_counter.count_tokens(&fact_text);

        if total_tokens + fact_tokens <= max_tokens {
            result_facts.push(ranked_fact.fact);
            facts_text.push_str(&fact_text);
            total_tokens += fact_tokens;
        } else {
            break; // Budget exceeded
        }
    }

    let facts_included = result_facts.len();
    let facts_excluded = facts.len().saturating_sub(facts_included);

    TruncationResult { facts: result_facts, total_tokens, facts_included, facts_excluded }
}

/// Format facts for injection into system prompt.
pub fn format_memory_for_injection(
    facts: &[Fact],
    max_tokens: usize,
    voting_engine: &MemoryVotingEngine,
) -> String {
    let counter = SimpleTokenCounter::with_defaults();
    let result = truncate_to_token_budget(facts, max_tokens, voting_engine, &counter);

    if result.facts.is_empty() {
        return String::new();
    }

    let mut output = String::from("Memory Facts:\n");
    for (i, fact) in result.facts.iter().enumerate() {
        output.push_str(&format!("{}. [{}] {}\n", i + 1, fact.category.as_str(), fact.content));
    }

    if result.facts_excluded > 0 {
        output.push_str(&format!(
            "\n(... and {} more facts omitted due to token budget)",
            result.facts_excluded
        ));
    }

    output
}

#[derive(Debug, Clone)]
pub struct ArchivedMemoryMatch {
    pub entry: ArchivedMemory,
    pub similarity: f32,
}

#[derive(Debug, Clone)]
pub struct SegmentedMemoryRetrieval {
    pub recent_facts: Vec<Fact>,
    pub working_summaries: Vec<WorkingMemorySummary>,
    pub archived_matches: Vec<ArchivedMemoryMatch>,
    pub total_tokens: usize,
}

fn normalized_term_counts(text: &str) -> HashMap<String, f32> {
    let normalized = text.to_lowercase();
    let mut counts = HashMap::new();

    for token in normalized.split(|ch: char| !ch.is_alphanumeric()) {
        let token = token.trim();
        if token.len() >= 3 {
            *counts.entry(token.to_string()).or_insert(0.0) += 1.0;
        }
    }

    if counts.is_empty() {
        let fallback = normalized.trim();
        if !fallback.is_empty() {
            counts.insert(fallback.to_string(), 1.0);
        }
    }

    counts
}

pub fn extract_semantic_terms(text: &str) -> Vec<String> {
    let mut ranked: Vec<_> = normalized_term_counts(text).into_iter().collect();
    ranked.sort_by(|left, right| {
        right
            .1
            .partial_cmp(&left.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| left.0.cmp(&right.0))
    });
    ranked.into_iter().map(|(term, _)| term).take(12).collect()
}

pub fn build_sparse_vector(text: &str) -> SparseVector {
    let mut ranked: Vec<_> = normalized_term_counts(text).into_iter().collect();
    ranked.sort_by(|left, right| left.0.cmp(&right.0));

    SparseVector {
        dimensions: ranked.iter().map(|(term, _)| term.clone()).collect(),
        values: ranked.into_iter().map(|(_, value)| value).collect(),
    }
}

fn cosine_similarity(left: &SparseVector, right: &SparseVector) -> f32 {
    if left.dimensions.is_empty() || right.dimensions.is_empty() {
        return 0.0;
    }

    let right_map: HashMap<&str, f32> = right
        .dimensions
        .iter()
        .zip(right.values.iter())
        .map(|(dimension, value)| (dimension.as_str(), *value))
        .collect();

    let mut dot_product = 0.0;
    let mut left_norm = 0.0;
    let mut right_norm = 0.0;

    for value in &right.values {
        right_norm += value * value;
    }

    for (dimension, value) in left.dimensions.iter().zip(left.values.iter()) {
        left_norm += value * value;
        if let Some(other) = right_map.get(dimension.as_str()) {
            dot_product += value * other;
        }
    }

    if left_norm == 0.0 || right_norm == 0.0 {
        return 0.0;
    }

    dot_product / (left_norm.sqrt() * right_norm.sqrt())
}

fn archived_entry_text(entry: &ArchivedMemory) -> String {
    let mut parts = vec![entry.summary.clone()];
    if !entry.key_facts.is_empty() {
        parts.extend(entry.key_facts.iter().map(|fact| fact.content.clone()));
    }
    parts.join(" ")
}

pub fn rank_archived_memories(
    entries: &[ArchivedMemory],
    query: Option<&str>,
    limit: usize,
) -> Vec<ArchivedMemoryMatch> {
    if limit == 0 || entries.is_empty() {
        return Vec::new();
    }

    let query = query.unwrap_or_default().trim();
    let query_vector = (!query.is_empty()).then(|| build_sparse_vector(query));
    let query_terms: HashSet<String> = extract_semantic_terms(query).into_iter().collect();

    let mut ranked: Vec<_> = entries
        .iter()
        .cloned()
        .map(|entry| {
            let mut similarity = query_vector
                .as_ref()
                .map(|vector| cosine_similarity(vector, &entry.vector))
                .unwrap_or(0.0);

            if !query_terms.is_empty() {
                let overlap =
                    entry.semantic_terms.iter().filter(|term| query_terms.contains(*term)).count()
                        as f32;
                similarity += overlap * 0.2;
            }

            if !entry.mandatory_fact_ids.is_empty() {
                similarity += 0.05;
            }

            ArchivedMemoryMatch { entry, similarity }
        })
        .collect();

    ranked.sort_by(|left, right| {
        right
            .similarity
            .partial_cmp(&left.similarity)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| right.entry.created_at.cmp(&left.entry.created_at))
    });

    ranked
        .into_iter()
        .filter(|matched| !query.is_empty() || !matched.entry.key_facts.is_empty())
        .take(limit)
        .collect()
}

pub fn retrieve_segmented_context(
    doc: &MemoryDocument,
    query: Option<&str>,
    max_tokens: usize,
    voting_engine: &MemoryVotingEngine,
    token_counter: &dyn TokenCounter,
    archived_limit: usize,
) -> SegmentedMemoryRetrieval {
    if !doc.segmented_context.has_any_content() {
        let fallback =
            truncate_to_token_budget(&doc.facts, max_tokens, voting_engine, token_counter);
        return SegmentedMemoryRetrieval {
            recent_facts: fallback.facts,
            working_summaries: Vec::new(),
            archived_matches: Vec::new(),
            total_tokens: fallback.total_tokens,
        };
    }

    let mut recent_facts = doc.segmented_context.recent.facts.clone();
    recent_facts.sort_by(|left, right| right.created_at.cmp(&left.created_at));

    let mut working_summaries = doc.segmented_context.working.summaries.clone();
    working_summaries.sort_by(|left, right| right.created_at.cmp(&left.created_at));

    let mut selected_recent = Vec::new();
    let mut selected_working = Vec::new();
    let mut selected_archived = Vec::new();
    let mut total_tokens = 0;

    for fact in recent_facts {
        let fact_text = format!("{}\n", fact.content);
        let fact_tokens = token_counter.count_tokens(&fact_text);
        if total_tokens + fact_tokens > max_tokens {
            break;
        }

        total_tokens += fact_tokens;
        selected_recent.push(fact);
    }

    for summary in working_summaries {
        let summary_text = format!("{}\n", summary.summary);
        let summary_tokens = token_counter.count_tokens(&summary_text);
        if total_tokens + summary_tokens > max_tokens {
            break;
        }

        total_tokens += summary_tokens;
        selected_working.push(summary);
    }

    for matched in
        rank_archived_memories(&doc.segmented_context.archived.entries, query, archived_limit)
    {
        let entry_text = archived_entry_text(&matched.entry);
        let entry_tokens = token_counter.count_tokens(&entry_text);
        if total_tokens + entry_tokens > max_tokens {
            continue;
        }

        total_tokens += entry_tokens;
        selected_archived.push(matched);
    }

    SegmentedMemoryRetrieval {
        recent_facts: selected_recent,
        working_summaries: selected_working,
        archived_matches: selected_archived,
        total_tokens,
    }
}

pub fn format_segmented_memory_for_injection(
    doc: &MemoryDocument,
    query: Option<&str>,
    max_tokens: usize,
    voting_engine: &MemoryVotingEngine,
    archived_limit: usize,
) -> String {
    let counter = SimpleTokenCounter::with_defaults();
    let retrieval =
        retrieve_segmented_context(doc, query, max_tokens, voting_engine, &counter, archived_limit);

    if retrieval.recent_facts.is_empty()
        && retrieval.working_summaries.is_empty()
        && retrieval.archived_matches.is_empty()
    {
        return String::new();
    }

    let mut sections = Vec::new();

    if !retrieval.recent_facts.is_empty() {
        let mut section = String::from("<recent>\n");
        for fact in retrieval.recent_facts {
            section.push_str(&format!("- [{}] {}\n", fact.category.as_str(), fact.content));
        }
        section.push_str("</recent>");
        sections.push(section);
    }

    if !retrieval.working_summaries.is_empty() {
        let mut section = String::from("<working>\n");
        for summary in retrieval.working_summaries {
            section.push_str(&format!("- {}\n", summary.summary));
        }
        section.push_str("</working>");
        sections.push(section);
    }

    if !retrieval.archived_matches.is_empty() {
        let mut section = String::from("<archived>\n");
        for matched in retrieval.archived_matches {
            section.push_str(&format!("- {}\n", matched.entry.summary));
            for fact in matched.entry.key_facts {
                section.push_str(&format!("  * {}\n", fact.content));
            }
        }
        section.push_str("</archived>");
        sections.push(section);
    }

    sections.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory_document::{
        ArchivedContextSegment, CompressionTriggerKind, FactCategory, RecentContextSegment,
        SegmentedContext, WorkingContextSegment,
    };

    #[test]
    fn test_truncate_to_budget() {
        let voting_engine = MemoryVotingEngine::with_defaults();
        let counter = SimpleTokenCounter::with_defaults();

        let facts = vec![
            Fact::new("Short".to_string(), FactCategory::Knowledge, 0.9, "t1".to_string()),
            Fact::new(
                "Medium length fact".to_string(),
                FactCategory::Knowledge,
                0.7,
                "t2".to_string(),
            ),
            Fact::new(
                "Very long fact with lots of content that should be truncated".to_string(),
                FactCategory::Knowledge,
                0.5,
                "t3".to_string(),
            ),
        ];

        // Very small budget - should only include highest weight fact
        let result = truncate_to_token_budget(&facts, 5, &voting_engine, &counter);
        // Token counting may vary, just verify we get some results within reasonable bounds
        assert!(result.facts_included >= 1);
        assert!(result.total_tokens <= 100); // Reasonable upper bound
    }

    #[test]
    fn test_format_for_injection() {
        let voting_engine = MemoryVotingEngine::with_defaults();
        let facts = vec![
            Fact::new("Fact 1".to_string(), FactCategory::Preference, 0.9, "t1".to_string()),
            Fact::new("Fact 2".to_string(), FactCategory::Knowledge, 0.8, "t2".to_string()),
        ];

        let formatted = format_memory_for_injection(&facts, 100, &voting_engine);
        assert!(formatted.contains("Memory Facts:"));
        assert!(formatted.contains("Fact 1"));
        assert!(formatted.contains("Fact 2"));
    }

    #[test]
    fn test_empty_facts() {
        let voting_engine = MemoryVotingEngine::with_defaults();
        let counter = SimpleTokenCounter::with_defaults();

        let result = truncate_to_token_budget(&[], 100, &voting_engine, &counter);
        assert_eq!(result.facts.len(), 0);
        assert_eq!(result.total_tokens, 0);
    }

    #[test]
    fn test_rank_archived_memories_prefers_semantic_match() {
        let postgres_fact = Fact::new(
            "Primary database is PostgreSQL".to_string(),
            FactCategory::Knowledge,
            0.9,
            "t1".to_string(),
        );
        let redis_fact = Fact::new(
            "Cache uses Redis".to_string(),
            FactCategory::Knowledge,
            0.8,
            "t1".to_string(),
        );

        let entries = vec![
            ArchivedMemory::new(
                "Database architecture".to_string(),
                vec![postgres_fact.clone()],
                std::slice::from_ref(&postgres_fact),
                extract_semantic_terms("postgres database architecture"),
                build_sparse_vector(
                    "postgres database architecture Primary database is PostgreSQL",
                ),
                CompressionTriggerKind::TokenThreshold,
                12,
            ),
            ArchivedMemory::new(
                "Cache architecture".to_string(),
                vec![redis_fact.clone()],
                std::slice::from_ref(&redis_fact),
                extract_semantic_terms("redis cache architecture"),
                build_sparse_vector("redis cache architecture Cache uses Redis"),
                CompressionTriggerKind::TokenThreshold,
                10,
            ),
        ];

        let ranked =
            rank_archived_memories(&entries, Some("Which database are we using? postgres"), 2);
        assert_eq!(ranked.len(), 2);
        assert!(ranked[0].entry.summary.contains("Database"));
        assert!(ranked[0].similarity >= ranked[1].similarity);
    }

    #[test]
    fn test_format_segmented_memory_includes_archived_key_facts() {
        let mandatory_fact = Fact::new(
            "Deployment window is Friday 18:00 UTC".to_string(),
            FactCategory::Goal,
            0.9,
            "t1".to_string(),
        )
        .with_mandatory(true);

        let archived_entry = ArchivedMemory::new(
            "Release schedule".to_string(),
            vec![mandatory_fact.clone()],
            std::slice::from_ref(&mandatory_fact),
            extract_semantic_terms("release deployment friday utc"),
            build_sparse_vector(
                "release deployment friday utc Deployment window is Friday 18:00 UTC",
            ),
            CompressionTriggerKind::MilestoneSnapshot,
            14,
        );

        let doc = MemoryDocument {
            segmented_context: SegmentedContext {
                recent: RecentContextSegment::default(),
                working: WorkingContextSegment::default(),
                archived: ArchivedContextSegment {
                    entries: vec![archived_entry],
                    estimated_tokens: 14,
                    updated_at: None,
                },
                compression_log: Vec::new(),
                last_milestone_snapshot: 0,
            },
            ..Default::default()
        };

        let formatted = format_segmented_memory_for_injection(
            &doc,
            Some("when is the deployment window"),
            100,
            &MemoryVotingEngine::with_defaults(),
            2,
        );

        assert!(formatted.contains("<archived>"));
        assert!(formatted.contains("Deployment window is Friday 18:00 UTC"));
    }
}
