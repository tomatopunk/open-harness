use thiserror::Error;

#[derive(Debug, Error)]
pub enum GovernanceError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("yaml: {0}")]
    Yaml(String),
    #[error("figment: {0}")]
    Figment(String),
}

pub type GovernanceResult<T> = Result<T, GovernanceError>;
