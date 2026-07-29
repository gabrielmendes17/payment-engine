//! Small internal helpers shared by the use cases.

use std::fmt::{Debug, Display};

use crate::application::errors::EngineError;
use crate::application::ports::outbound::PaymentRepository;
use crate::domain::{Account, ClientId};

/// Load the client's account or synthesize a fresh zero-balance account.
/// Used by primary operations (deposit / withdrawal) which may target a
/// previously-unknown client.
pub(crate) fn load_or_new_account<R>(
    repository: &R,
    client: ClientId,
) -> Result<Account, EngineError<R::Error>>
where
    R: PaymentRepository,
    R::Error: Debug + Display,
{
    Ok(repository
        .account(client)
        .map_err(EngineError::Repository)?
        .unwrap_or_else(|| Account::new(client)))
}

/// Load the client's account or return an `InvariantViolation`. Used by
/// lifecycle operations (dispute / resolve / chargeback) which require a
/// pre-existing account because the deposit they reference implies one.
pub(crate) fn require_account<R>(
    repository: &R,
    client: ClientId,
) -> Result<Account, EngineError<R::Error>>
where
    R: PaymentRepository,
    R::Error: Debug + Display,
{
    repository
        .account(client)
        .map_err(EngineError::Repository)?
        .ok_or(EngineError::InvariantViolation(
            "deposit exists without owning account",
        ))
}

/// Map a repository error into `EngineError::Repository`.
pub(crate) fn repo_err<E>(e: E) -> EngineError<E>
where
    E: Debug + Display,
{
    EngineError::Repository(e)
}
