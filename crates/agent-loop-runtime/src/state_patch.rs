//! Batched state updates as an explicit patch type (channel-style writes before reducer merge).

use crate::turn_reducer::{apply_turn_effects, TurnEffect};
use agent_ports::ThreadState;

/// Ordered list of turn-scoped effects applied atomically to [`ThreadState`].
#[derive(Debug, Clone, Default)]
pub struct StatePatch(pub Vec<TurnEffect>);

impl StatePatch {
    /// Apply all effects in order.
    pub fn apply(self, state: &mut ThreadState) {
        apply_turn_effects(state, &self.0);
    }

    /// Merge another patch after this one (concatenation).
    #[must_use]
    pub fn then(mut self, other: StatePatch) -> Self {
        self.0.extend(other.0);
        self
    }
}

impl From<Vec<TurnEffect>> for StatePatch {
    fn from(effects: Vec<TurnEffect>) -> Self {
        Self(effects)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::turn_reducer::TurnEffect;
    use agent_ports::ThreadId;

    #[test]
    fn patch_then_apply() {
        let tid = ThreadId::new_v4();
        let mut st = agent_ports::ThreadState::new(tid);
        let p = StatePatch(vec![TurnEffect::SetClarification { prompt: Some("p".into()) }]);
        p.apply(&mut st);
        assert!(st.clarification_state.pending);
    }
}
