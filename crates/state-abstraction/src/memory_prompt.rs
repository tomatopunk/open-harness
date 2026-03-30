//! Prompt templates for LLM-based memory extraction.
//!
//! This module is deprecated. Use `crate::memory::prompts` instead.

#[deprecated(
    since = "0.2.0",
    note = "Use crate::memory::prompts::MEMORY_UPDATE_PROMPT instead"
)]
pub use crate::memory::prompts::MEMORY_UPDATE_PROMPT;

#[deprecated(
    since = "0.2.0",
    note = "Use crate::memory::prompts::MERGE_PROFILE_PROMPT instead"
)]
pub use crate::memory::prompts::MERGE_PROFILE_PROMPT;

// Re-export formatting functions for backward compatibility
pub use crate::memory_document::{Fact, MemoryDocument};

/// Formats a fact for inclusion in prompts.
#[must_use]
pub fn format_fact(fact: &Fact) -> String {
    let timestamp = fact.created_at.format("%Y-%m-%d");
    format!(
        "[{} | {} | confidence: {:.2}] {}",
        fact.category_str(),
        timestamp,
        fact.confidence,
        fact.content
    )
}

/// Formats multiple facts for inclusion in prompts.
#[must_use]
pub fn format_facts(facts: &[Fact]) -> String {
    facts.iter().map(format_fact).collect::<Vec<_>>().join("\n")
}

/// Formats a memory document for inclusion in prompts.
#[must_use]
pub fn format_memory_for_prompt(doc: &MemoryDocument) -> String {
    let mut parts = Vec::new();

    if let Some(ref work) = doc.user.work_context {
        parts.push(format!("Work Context: {}", work));
    }
    if let Some(ref personal) = doc.user.personal_context {
        parts.push(format!("Personal Context: {}", personal));
    }
    if let Some(ref top_of_mind) = doc.user.top_of_mind {
        parts.push(format!("Top of Mind: {}", top_of_mind));
    }
    if let Some(ref recent) = doc.history.recent_months {
        parts.push(format!("Recent Months: {}", recent));
    }
    if let Some(ref earlier) = doc.history.earlier_context {
        parts.push(format!("Earlier Context: {}", earlier));
    }
    if let Some(ref long_term) = doc.history.long_term_background {
        parts.push(format!("Long-term Background: {}", long_term));
    }
    if !doc.facts.is_empty() {
        parts.push("Facts:".to_string());
        parts.push(format_facts(&doc.facts));
    }

    parts.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory_document::{Fact, FactCategory};

    #[test]
    fn test_format_fact() {
        let fact = Fact::new(
            "用户喜欢 Rust".to_string(),
            FactCategory::Preference,
            0.9,
            "thread-1".to_string(),
        );

        let formatted = format_fact(&fact);
        assert!(formatted.contains("preference"));
        assert!(formatted.contains("用户喜欢 Rust"));
        assert!(formatted.contains("confidence: 0.90"));
    }

    #[test]
    fn test_prompt_templates_exist() {
        #[allow(deprecated)]
        {
            assert!(MEMORY_UPDATE_PROMPT.contains("<current_memory>"));
            assert!(MEMORY_UPDATE_PROMPT.contains("newFacts"));
            assert!(MERGE_PROFILE_PROMPT.contains("<facts>"));
        }
    }
}
