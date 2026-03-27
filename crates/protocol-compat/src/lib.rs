//! LangGraph-compatible protocol types for open-harness.
//!
//! Freeze contract: threads, runs, stream modes, SSE event shapes.

pub mod configurable;
pub mod error;
pub mod openai;
pub mod sse;
pub mod threads;

pub use configurable::Configurable;
pub use error::ProtocolError;
pub use openai::{
    OpenAiChatCompletionsRequest, OpenAiChatMessage, OpenAiModelItem, OpenAiModelsResponse,
};
pub use sse::{SseEvent, StreamMode};
pub use threads::{RunRequest, ThreadCreate};
