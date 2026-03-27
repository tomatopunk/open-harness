use thiserror::Error;

#[derive(Debug, Error)]
pub enum ModelError {
    #[error("openai: {0}")]
    OpenAi(String),
    #[error("config: {0}")]
    Config(String),
}
