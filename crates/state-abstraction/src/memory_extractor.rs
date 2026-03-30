//! LLM-based fact extractor for memory updates.

use crate::memory_document::{Fact, FactCategory, MemoryDocument};
use crate::memory_prompt;
use agent_ports::{LLMPort, LlmTurnContext};
use serde::{Deserialize, Serialize};

/// Extracted memory update from LLM analysis.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryUpdate {
    pub user: UserUpdate,
    pub history: HistoryUpdate,
    pub new_facts: Vec<NewFact>,
    pub facts_to_remove: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserUpdate {
    pub work_context: Option<ContextUpdate>,
    pub personal_context: Option<ContextUpdate>,
    pub top_of_mind: Option<ContextUpdate>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextUpdate {
    pub summary: String,
    pub should_update: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryUpdate {
    pub recent_months: Option<ContextUpdate>,
    pub earlier_context: Option<ContextUpdate>,
    pub long_term_background: Option<ContextUpdate>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewFact {
    pub content: String,
    pub category: String,
    pub confidence: f32,
}

/// Fact extractor using LLM to analyze conversations.
pub struct FactExtractor {
    llm_port: Box<dyn LLMPort>,
    confidence_threshold: f32,
}

impl FactExtractor {
    /// Create a new fact extractor.
    pub fn new(llm_port: Box<dyn LLMPort>, confidence_threshold: f32) -> Self {
        Self { llm_port, confidence_threshold }
    }

    /// Extract memory updates from a conversation.
    pub async fn extract(
        &self,
        current_memory: &MemoryDocument,
        conversation: &str,
    ) -> Result<MemoryUpdate, String> {
        // Format current memory
        let current_memory_str = memory_prompt::format_memory_for_prompt(current_memory);

        // Build prompt
        let prompt = memory_prompt::MEMORY_UPDATE_PROMPT
            .replace("{current_memory}", &current_memory_str)
            .replace("{conversation}", conversation);

        // Call LLM
        let response = self
            .llm_port
            .infer_turn(LlmTurnContext {
                messages: vec![serde_json::json!({
                    "role": "user",
                    "content": prompt
                })],
                ..Default::default()
            })
            .await
            .map_err(|e| format!("LLM call failed: {}", e))?;

        // Parse response
        let response_text =
            response.assistant_text.ok_or_else(|| "No response from LLM".to_string())?;

        let update: MemoryUpdate = serde_json::from_str(&response_text)
            .map_err(|e| format!("Failed to parse LLM response: {}", e))?;

        Ok(update)
    }

    /// Convert extracted update to Facts with filtering.
    pub fn process_update(&self, update: MemoryUpdate, source_thread: String) -> ProcessedUpdate {
        let mut new_facts = Vec::new();

        for fact in update.new_facts {
            // Filter by confidence threshold
            if fact.confidence < self.confidence_threshold {
                continue;
            }

            // Parse category
            let category = match fact.category.to_lowercase().as_str() {
                "preference" => FactCategory::Preference,
                "knowledge" => FactCategory::Knowledge,
                "context" => FactCategory::Context,
                "behavior" => FactCategory::Behavior,
                "goal" => FactCategory::Goal,
                _ => FactCategory::Knowledge, // Default
            };

            new_facts.push(Fact::new(
                fact.content,
                category,
                fact.confidence,
                source_thread.clone(),
            ));
        }

        ProcessedUpdate {
            new_facts,
            facts_to_remove: update.facts_to_remove,
            user_update: update.user,
            history_update: update.history,
        }
    }
}

/// Processed update ready to be applied to MemoryDocument.
#[derive(Debug, Clone)]
pub struct ProcessedUpdate {
    pub new_facts: Vec<Fact>,
    pub facts_to_remove: Vec<String>,
    pub user_update: UserUpdate,
    pub history_update: HistoryUpdate,
}

impl ProcessedUpdate {
    /// Apply this update to a MemoryDocument.
    pub fn apply(self, doc: &mut MemoryDocument) {
        // Remove facts
        doc.remove_facts(&self.facts_to_remove);

        // Add new facts
        for fact in self.new_facts {
            doc.add_fact(fact);
        }

        // Update user context
        if let Some(work) = self.user_update.work_context {
            if work.should_update {
                doc.user.work_context = Some(work.summary);
                doc.user.updated_at = Some(chrono::Utc::now());
            }
        }
        if let Some(personal) = self.user_update.personal_context {
            if personal.should_update {
                doc.user.personal_context = Some(personal.summary);
                doc.user.updated_at = Some(chrono::Utc::now());
            }
        }
        if let Some(top_of_mind) = self.user_update.top_of_mind {
            if top_of_mind.should_update {
                doc.user.top_of_mind = Some(top_of_mind.summary);
                doc.user.updated_at = Some(chrono::Utc::now());
            }
        }

        // Update history
        if let Some(recent) = self.history_update.recent_months {
            if recent.should_update {
                doc.history.recent_months = Some(recent.summary);
                doc.history.updated_at = Some(chrono::Utc::now());
            }
        }
        if let Some(earlier) = self.history_update.earlier_context {
            if earlier.should_update {
                doc.history.earlier_context = Some(earlier.summary);
                doc.history.updated_at = Some(chrono::Utc::now());
            }
        }
        if let Some(long_term) = self.history_update.long_term_background {
            if long_term.should_update {
                doc.history.long_term_background = Some(long_term.summary);
                doc.history.updated_at = Some(chrono::Utc::now());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_processed_update_apply() {
        let mut doc = MemoryDocument::default();

        let update = ProcessedUpdate {
            new_facts: vec![Fact::new(
                "Test fact".to_string(),
                FactCategory::Knowledge,
                0.9,
                "thread-1".to_string(),
            )],
            facts_to_remove: vec![],
            user_update: UserUpdate {
                work_context: Some(ContextUpdate {
                    summary: "New work".to_string(),
                    should_update: true,
                }),
                personal_context: None,
                top_of_mind: None,
            },
            history_update: HistoryUpdate {
                recent_months: None,
                earlier_context: None,
                long_term_background: None,
            },
        };

        update.apply(&mut doc);

        assert_eq!(doc.facts.len(), 1);
        assert_eq!(doc.user.work_context, Some("New work".to_string()));
        assert!(doc.user.updated_at.is_some());
    }
}
