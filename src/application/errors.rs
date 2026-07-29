use std::fmt::{Debug, Display};

use thiserror::Error;

#[derive(Debug, Error)]
pub enum EngineError<E>
where
    E: Debug + Display,
{
    #[error("repository error: {0}")]
    Repository(#[source] E),

    #[error("invariant violation: {0}")]
    InvariantViolation(&'static str),
}
