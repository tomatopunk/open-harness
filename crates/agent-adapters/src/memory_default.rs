//! Default memory port: snippet passthrough + fact extraction from messages.

use agent_ports::{MemoryContext, MemoryDelta, MemoryPort, PortResult, ThreadState};
use async_trait::async_trait;

#[derive(Debug, Default, Clone)]
pub struct DefaultMemoryAdapter;

#[async_trait]
impl MemoryPort for DefaultMemoryAdapter {
    async fn retrieve(&self, ctx: &MemoryContext) -> PortResult<Vec<String>> {
        Ok(ctx.state.memory_working_set.snippets.clone())
    }

    async fn extract_and_commit(&self, state: &mut ThreadState) -> PortResult<MemoryDelta> {
        let mut facts = Vec::new();
        for m in &state.messages {
            if let Some(s) = m.content.as_str() {
                for line in s.lines() {
                    let line = line.trim();
                    if let Some(rest) = line.strip_prefix("fact:") {
                        facts.push(rest.trim().to_string());
                    }
                }
            }
        }
        if !facts.is_empty() {
            state.memory_commits.push(agent_ports::MemoryCommit {
                facts: facts.clone(),
                committed_at: chrono::Utc::now(),
            });
        }
        Ok(MemoryDelta { facts, snippets: Vec::new() })
    }
}
