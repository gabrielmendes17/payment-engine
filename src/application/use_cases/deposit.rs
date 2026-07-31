use rust_decimal::Decimal;

use crate::application::changes::LedgerChanges;
use crate::application::errors::EngineError;
use crate::application::helpers::load_or_new_account;
use crate::application::outcome::{ApplyOutcome, RejectionReason};
use crate::application::ports::outbound::LedgerRepository;
use crate::domain::{ClientId, Deposit, TransactionId};

/// A rejected primary tx still reserves its id — a tx number is a one-shot
/// identifier regardless of outcome.
pub(crate) fn execute<R>(
    repository: &mut R,
    client: ClientId,
    tx: TransactionId,
    amount: Decimal,
) -> Result<ApplyOutcome, EngineError<R::Error>>
where
    R: LedgerRepository,
{
    if repository
        .transaction_seen(tx)
        .map_err(EngineError::Repository)?
    {
        return Ok(ApplyOutcome::Rejected(
            RejectionReason::DuplicateTransaction { tx },
        ));
    }

    let account = load_or_new_account(repository, client)?;
    // Clone so the rejection branch can persist the unmutated account —
    // a later lifecycle event on a first-time client would otherwise fail
    // with "deposit exists without owning account".
    match account.clone().credit(amount) {
        Ok(updated) => {
            let deposit = Deposit::new(tx, client, amount).map_err(|_| {
                EngineError::InvariantViolation(
                    "credit succeeded but deposit construction rejected the amount",
                )
            })?;
            let changes = LedgerChanges::new(updated)
                .reserving(tx)
                .with_deposit(deposit);
            repository
                .commit(changes)
                .map_err(EngineError::Repository)?;
            Ok(ApplyOutcome::Applied)
        }
        Err(reason) => {
            repository
                .commit(LedgerChanges::new(account).reserving(tx))
                .map_err(EngineError::Repository)?;
            Ok(ApplyOutcome::Rejected(reason.into()))
        }
    }
}
