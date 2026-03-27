//! Model provider abstraction (OpenAI-compatible via async-openai).

pub mod error;
pub mod traits;

pub use error::ModelError;
pub use traits::ChatModel;
