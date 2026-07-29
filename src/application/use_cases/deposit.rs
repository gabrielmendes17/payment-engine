use std::fmt::{Debug, Display};

use rust_decimal::Decimal;

use crate::application::changes::{DepositChange, LedgerChanges};
use crate::application::errors::EngineError;
use crate::application::helpers::{load_or_new_account, repo_err};
use crate::application::ports::outbound::PaymentRepository;
use crate::domain::{ApplyOutcome, ClientId, DepositRecord, RejectionReason, TransactionId};

/// Deposit use case.
///
/// - Reject duplicate `tx` before touching state.
/// - For any domain rejection on the primary path, still reserve `tx` so it
///   cannot be reused later. This matches specs/02-processing-rules.md.
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
    match account.credit(amount) {
        Ok(updated) => {
            let deposit = DepositRecord::new_applied(tx, client, amount);
            let changes = LedgerChanges::new()
                .reserving(tx)
                .with_account(updated)
                .with_deposit(DepositChange::Insert(deposit));
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
