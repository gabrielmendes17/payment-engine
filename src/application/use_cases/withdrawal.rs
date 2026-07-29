use std::fmt::{Debug, Display};

use rust_decimal::Decimal;

use crate::application::changes::LedgerChanges;
use crate::application::errors::EngineError;
use crate::application::helpers::{load_or_new_account, repo_err};
use crate::application::ports::outbound::PaymentRepository;
use crate::domain::{ApplyOutcome, ClientId, RejectionReason, TransactionId};

/// Withdrawal use case. Same tx-reservation rules as deposit.
pub fn run<R>(
    repository: &mut R,
    client: ClientId,
    tx: TransactionId,
    amount: Decimal,
) -> Result<ApplyOutcome, EngineError<R::Error>>
where
    R: PaymentRepository,
    R::Error: Debug + Display,
{
    if repository.transaction_seen(tx).map_err(repo_err)? {
        return Ok(ApplyOutcome::Rejected(
            RejectionReason::DuplicateTransaction { tx },
        ));
    }

    let account = load_or_new_account(repository, client)?;
    match account.debit(amount) {
        Ok(updated) => {
            let changes = LedgerChanges::new().reserving(tx).with_account(updated);
            repository.commit(changes).map_err(repo_err)?;
            Ok(ApplyOutcome::Applied)
        }
        Err(reason) => {
            repository
                .commit(LedgerChanges::new().reserving(tx))
                .map_err(repo_err)?;
            Ok(ApplyOutcome::Rejected(reason))
        }
    }
}
