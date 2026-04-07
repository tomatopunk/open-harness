//! Memory injection utilities for LLM prompts.

use crate::memory_document::MemoryDocument;
use crate::memory_retrieval::{format_memory_for_injection, format_segmented_memory_for_injection};
use crate::memory_voting::MemoryVotingEngine;

/// Memory injection configuration.
#[derive(Debug, Clone)]
pub struct MemoryInjectionConfig {
    pub max_tokens: usize,
    pub include_user_context: bool,
    pub include_history: bool,
    pub include_facts: bool,
    pub facts_only: bool,
    pub archived_retrieval_limit: usize,
}

impl Default for MemoryInjectionConfig {
    fn default() -> Self {
        Self {
            max_tokens: 2000,
            include_user_context: true,
            include_history: true,
            include_facts: true,
            facts_only: false,
            archived_retrieval_limit: 4,
        }
    }
}

/// Format complete memory for injection into system prompt.
pub fn format_complete_memory(
    doc: &MemoryDocument,
    config: &MemoryInjectionConfig,
    voting_engine: &MemoryVotingEngine,
) -> String {
    format_complete_memory_with_query(doc, config, voting_engine, None)
}

pub fn format_complete_memory_with_query(
    doc: &MemoryDocument,
    config: &MemoryInjectionConfig,
    voting_engine: &MemoryVotingEngine,
    query: Option<&str>,
) -> String {
    if config.facts_only {
        if doc.segmented_context.has_any_content() {
            return format_segmented_memory_for_injection(
                doc,
                query,
                config.max_tokens,
                voting_engine,
                config.archived_retrieval_limit,
            );
        }

        return format_memory_for_injection(&doc.facts, config.max_tokens, voting_engine);
    }

    let mut parts = Vec::new();

    // User context
    if config.include_user_context {
        if let Some(ref work) = doc.user.work_context {
            parts.push(format!("<work_context>{}</work_context>", work));
        }
        if let Some(ref personal) = doc.user.personal_context {
            parts.push(format!("<personal_context>{}</personal_context>", personal));
        }
        if let Some(ref top_of_mind) = doc.user.top_of_mind {
            parts.push(format!("<top_of_mind>{}</top_of_mind>", top_of_mind));
        }
    }

    // History
    if config.include_history {
        if let Some(ref recent) = doc.history.recent_months {
            parts.push(format!("<recent_months>{}</recent_months>", recent));
        }
        if let Some(ref earlier) = doc.history.earlier_context {
            parts.push(format!("<earlier_context>{}</earlier_context>", earlier));
        }
        if let Some(ref long_term) = doc.history.long_term_background {
            parts.push(format!("<long_term_background>{}</long_term_background>", long_term));
        }
    }

    // Facts
    if config.include_facts {
        let facts_str = if doc.segmented_context.has_any_content() {
            format_segmented_memory_for_injection(
                doc,
                query,
                config.max_tokens,
                voting_engine,
                config.archived_retrieval_limit,
            )
        } else {
            format_memory_for_injection(&doc.facts, config.max_tokens, voting_engine)
        };
        if !facts_str.is_empty() {
            parts.push(format!("<facts>\n{}</facts>", facts_str));
        }
    }

    if parts.is_empty() {
        return String::new();
    }

    format!("<memory>\n{}\n</memory>", parts.join("\n"))
}

/// Inject memory into an existing system prompt.
pub fn inject_memory_to_prompt(
    system_prompt: &str,
    memory: &MemoryDocument,
    config: &MemoryInjectionConfig,
    voting_engine: &MemoryVotingEngine,
) -> String {
    inject_memory_to_prompt_with_query(system_prompt, memory, config, voting_engine, None)
}

pub fn inject_memory_to_prompt_with_query(
    system_prompt: &str,
    memory: &MemoryDocument,
    config: &MemoryInjectionConfig,
    voting_engine: &MemoryVotingEngine,
    query: Option<&str>,
) -> String {
    let memory_text = format_complete_memory_with_query(memory, config, voting_engine, query);

    if memory_text.is_empty() {
        return system_prompt.to_string();
    }

    format!("{}\n\n{}", system_prompt, memory_text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory_document::{Fact, FactCategory};

    #[test]
    fn test_format_complete_memory() {
        let mut doc = MemoryDocument::default();
        doc.user.work_context = Some("Software engineer".to_string());
        doc.add_fact(Fact::new(
            "Likes Python".to_string(),
            FactCategory::Preference,
            0.9,
            "t1".to_string(),
        ));

        let config = MemoryInjectionConfig::default();
        let voting_engine = MemoryVotingEngine::with_defaults();

        let formatted = format_complete_memory(&doc, &config, &voting_engine);
        assert!(formatted.contains("<memory>"));
        assert!(formatted.contains("<work_context>"));
        assert!(formatted.contains("Software engineer"));
        assert!(formatted.contains("Likes Python"));
    }

    #[test]
    fn test_inject_to_prompt() {
        let mut doc = MemoryDocument::default();
        doc.add_fact(Fact::new(
            "Test fact".to_string(),
            FactCategory::Knowledge,
            0.9,
            "t1".to_string(),
        ));

        let config = MemoryInjectionConfig::default();
        let voting_engine = MemoryVotingEngine::with_defaults();

        let prompt =
            inject_memory_to_prompt("You are a helpful assistant.", &doc, &config, &voting_engine);
        assert!(prompt.contains("You are a helpful assistant."));
        assert!(prompt.contains("<memory>"));
    }

    #[test]
    fn test_facts_only_mode() {
        let mut doc = MemoryDocument::default();
        doc.user.work_context = Some("Engineer".to_string());
        doc.add_fact(Fact::new("Fact".to_string(), FactCategory::Knowledge, 0.9, "t1".to_string()));

        let config = MemoryInjectionConfig { facts_only: true, ..Default::default() };
        let voting_engine = MemoryVotingEngine::with_defaults();

        let formatted = format_complete_memory(&doc, &config, &voting_engine);
        assert!(!formatted.contains("<work_context>"));
        assert!(formatted.contains("Fact"));
    }

    #[test]
    fn test_query_injection_uses_segmented_memory() {
        let mut doc = MemoryDocument::default();
        let archived_fact = Fact::new(
            "Primary database is PostgreSQL".to_string(),
            FactCategory::Knowledge,
            0.95,
            "t1".to_string(),
        );

        doc.segmented_context.archived.entries.push(crate::memory_document::ArchivedMemory::new(
            "Database history".to_string(),
            vec![archived_fact.clone()],
            std::slice::from_ref(&archived_fact),
            crate::memory_retrieval::extract_semantic_terms("postgres database"),
            crate::memory_retrieval::build_sparse_vector(
                "postgres database Primary database is PostgreSQL",
            ),
            crate::memory_document::CompressionTriggerKind::TokenThreshold,
            12,
        ));

        let config = MemoryInjectionConfig::default();
        let voting_engine = MemoryVotingEngine::with_defaults();

        let formatted = format_complete_memory_with_query(
            &doc,
            &config,
            &voting_engine,
            Some("which database do we use"),
        );

        assert!(formatted.contains("<archived>"));
        assert!(formatted.contains("Primary database is PostgreSQL"));
    }
}
