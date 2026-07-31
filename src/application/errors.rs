use thiserror::Error;

use crate::domain::ClientId;

#[derive(Debug, Error)]
pub enum EngineError<E>
where
    E: std::error::Error + Send + Sync + 'static,
{
    #[error("repository error: {0}")]
    Repository(#[source] E),

    #[error("invariant violation: {0}")]
    InvariantViolation(&'static str),

    #[error("balance arithmetic overflow for client {client}")]
    ArithmeticOverflow { client: ClientId },
}
