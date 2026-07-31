use crate::application::errors::EngineError;
use crate::application::ports::outbound::LedgerRepository;
use crate::domain::{Account, ClientId};

pub(crate) fn load_or_new_account<R>(
    repository: &R,
    client: ClientId,
) -> Result<Account, EngineError<R::Error>>
where
    R: LedgerRepository,
{
    Ok(repository
        .account(client)
        .map_err(EngineError::Repository)?
        .unwrap_or_else(|| Account::new(client)))
}

pub(crate) fn require_account<R>(
    repository: &R,
    client: ClientId,
) -> Result<Account, EngineError<R::Error>>
where
    R: LedgerRepository,
{
    repository
        .account(client)
        .map_err(EngineError::Repository)?
        .ok_or(EngineError::InvariantViolation(
            "deposit exists without owning account",
        ))
}
