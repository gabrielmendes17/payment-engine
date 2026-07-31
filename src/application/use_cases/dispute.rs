use crate::application::changes::LedgerChanges;
use crate::application::errors::EngineError;
use crate::application::helpers::{classify_dispute_error, require_account};
use crate::application::outcome::{ApplyOutcome, RejectionReason};
use crate::application::ports::outbound::LedgerRepository;
use crate::domain::services::dispute_service;
use crate::domain::{ClientId, TransactionId};

pub(crate) fn execute<R>(
    repository: &mut R,
    client: ClientId,
    tx: TransactionId,
) -> Result<ApplyOutcome, EngineError<R::Error>>
where
    R: LedgerRepository,
{
    let Some(deposit) = repository.deposit(tx).map_err(EngineError::Repository)? else {
        return Ok(ApplyOutcome::Rejected(RejectionReason::DepositNotFound {
            tx,
        }));
    };
    // Ownership check before loading the account: a cross-client dispute
    // must surface as ClientMismatch, not InvariantViolation.
    if let Err(reason) = deposit.ensure_owned_by(client) {
        return Ok(ApplyOutcome::Rejected(reason.into()));
    }
    let account = require_account(repository, client)?;

    match dispute_service::apply_dispute(account, deposit) {
        Ok((updated_account, updated_deposit)) => {
            let changes = LedgerChanges::new(updated_account).with_deposit(updated_deposit);
            repository
                .commit(changes)
                .map_err(EngineError::Repository)?;
            Ok(ApplyOutcome::Applied)
        }
        Err(err) => {
            let reason = classify_dispute_error(err)?;
            Ok(ApplyOutcome::Rejected(reason))
        }
    }
}
