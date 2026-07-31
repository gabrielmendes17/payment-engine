use rust_decimal::Decimal;

use crate::application::changes::LedgerChanges;
use crate::application::errors::EngineError;
use crate::application::helpers::{classify_account_error, load_or_new_account};
use crate::application::outcome::{ApplyOutcome, RejectionReason};
use crate::application::ports::outbound::LedgerRepository;
use crate::domain::{ClientId, TransactionId};

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
    match account.clone().debit(amount) {
        Ok(updated) => {
            let changes = LedgerChanges::new(updated).reserving(tx);
            repository
                .commit(changes)
                .map_err(EngineError::Repository)?;
            Ok(ApplyOutcome::Applied)
        }
        Err(err) => {
            let reason = classify_account_error(err)?;
            repository
                .commit(LedgerChanges::new(account).reserving(tx))
                .map_err(EngineError::Repository)?;
            Ok(ApplyOutcome::Rejected(reason))
        }
    }
}
