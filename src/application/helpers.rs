use crate::application::errors::EngineError;
use crate::application::outcome::RejectionReason;
use crate::application::ports::outbound::LedgerRepository;
use crate::domain::errors::{AccountError, DisputeError};
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

pub(crate) fn classify_account_error<E>(
    error: AccountError,
) -> Result<RejectionReason, EngineError<E>>
where
    E: std::error::Error + Send + Sync + 'static,
{
    match error {
        AccountError::InvalidAmount => Ok(RejectionReason::InvalidAmount),
        AccountError::Locked { client } => Ok(RejectionReason::AccountLocked { client }),
        AccountError::InsufficientFunds { client } => {
            Ok(RejectionReason::InsufficientFunds { client })
        }
        AccountError::InsufficientHeldFunds { client } => {
            Ok(RejectionReason::InsufficientHeldFunds { client })
        }
        AccountError::ArithmeticOverflow { client } => {
            Err(EngineError::ArithmeticOverflow { client })
        }
    }
}

pub(crate) fn classify_dispute_error<E>(
    error: DisputeError,
) -> Result<RejectionReason, EngineError<E>>
where
    E: std::error::Error + Send + Sync + 'static,
{
    match error {
        DisputeError::Account(error) => classify_account_error(error),
        DisputeError::Deposit(error) => Ok(error.into()),
    }
}
