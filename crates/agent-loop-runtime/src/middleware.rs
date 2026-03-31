//! Pluggable hooks around the inner loop (aligned with LangChain `AgentMiddleware` and DeerFlow middlewares).
//!
//! Order within one turn (see `ARCHITECTURE.md`): lead kernel → PreModel → **`before_model`** → Model
//! → PostModel → **`after_model`** → dispatch; each tool invoke runs **`before_tool_call`** /
//! **`after_tool_call`** (together: `around_tool_call` semantics).

pub mod builder;
pub mod clarification;
pub mod skill_injection;
pub mod tool_assembly;

use std::sync::Arc;

use crate::error::AgentLoopResult;
use crate::run_config::AgentLoopRunConfig;
use agent_ports::{LlmTurnOutput, ThreadId, ThreadState, ToolCallSpec};
use async_trait::async_trait;
use serde_json::Value;

use crate::budget::RunBudget;

/// Context for one turn of the agent loop.
#[derive(Debug, Clone)]
pub struct TurnContext {
    pub thread_id: ThreadId,
    pub run_id: agent_ports::RunId,
    pub run_cfg: AgentLoopRunConfig,
    pub budget: RunBudget,
}

#[async_trait]
pub trait AgentLoopMiddleware: Send + Sync {
    /// After PreModel (skills + memory on `state`), before `infer_turn`. May edit `messages_for_llm`.
    async fn before_model(
        &self,
        _ctx: &TurnContext,
        _state: &mut ThreadState,
        _messages_for_llm: &mut Vec<Value>,
    ) -> AgentLoopResult<()> {
        Ok(())
    }

    /// After `infer_turn`, inside the PostModel stage (before branching).
    async fn after_model(
        &self,
        _ctx: &TurnContext,
        _state: &mut ThreadState,
        _out: &LlmTurnOutput,
    ) -> AgentLoopResult<()> {
        Ok(())
    }

    /// Immediately before each allowed tool `invoke` (after assembly check).
    async fn before_tool_call(
        &self,
        _ctx: &TurnContext,
        _state: &mut ThreadState,
        _call: &ToolCallSpec,
    ) -> AgentLoopResult<()> {
        Ok(())
    }

    /// After each tool `invoke` (success or mapped error payload).
    async fn after_tool_call(
        &self,
        _ctx: &TurnContext,
        _state: &mut ThreadState,
        _call: &ToolCallSpec,
        _ok: bool,
        _payload: &Value,
    ) -> AgentLoopResult<()> {
        Ok(())
    }
}

/// Default no-op middleware chain.
#[derive(Default)]
pub struct NoopMiddleware;

#[async_trait]
impl AgentLoopMiddleware for NoopMiddleware {}

/// Ordered pipeline of middlewares (outer-to-inner: first layer runs first for `before_*`,
/// first layer runs first for `after_*` — same order as LangChain chain composition).
pub struct MiddlewareChain {
    pub layers: Vec<Arc<dyn AgentLoopMiddleware>>,
}

impl MiddlewareChain {
    #[must_use]
    pub fn new(layers: Vec<Arc<dyn AgentLoopMiddleware>>) -> Self {
        Self { layers }
    }
}

#[async_trait]
impl AgentLoopMiddleware for MiddlewareChain {
    async fn before_model(
        &self,
        ctx: &TurnContext,
        state: &mut ThreadState,
        messages_for_llm: &mut Vec<Value>,
    ) -> AgentLoopResult<()> {
        for layer in &self.layers {
            layer.before_model(ctx, state, messages_for_llm).await?;
        }
        Ok(())
    }

    async fn after_model(
        &self,
        ctx: &TurnContext,
        state: &mut ThreadState,
        out: &LlmTurnOutput,
    ) -> AgentLoopResult<()> {
        for layer in &self.layers {
            layer.after_model(ctx, state, out).await?;
        }
        Ok(())
    }

    async fn before_tool_call(
        &self,
        ctx: &TurnContext,
        state: &mut ThreadState,
        call: &ToolCallSpec,
    ) -> AgentLoopResult<()> {
        for layer in &self.layers {
            layer.before_tool_call(ctx, state, call).await?;
        }
        Ok(())
    }

    async fn after_tool_call(
        &self,
        ctx: &TurnContext,
        state: &mut ThreadState,
        call: &ToolCallSpec,
        ok: bool,
        payload: &Value,
    ) -> AgentLoopResult<()> {
        for layer in &self.layers {
            layer.after_tool_call(ctx, state, call, ok, payload).await?;
        }
        Ok(())
    }
}
