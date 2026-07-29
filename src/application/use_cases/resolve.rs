use std::fmt::{Debug, Display};

use crate::application::changes::{DepositChange, LedgerChanges};
use crate::application::errors::EngineError;
use crate::application::helpers::{repo_err, require_account};
use crate::application::ports::outbound::PaymentRepository;
use crate::domain::services::dispute_service;
use crate::domain::{ApplyOutcome, ClientId, DepositStatus, RejectionReason, TransactionId};

/// Resolve use case. No tx reservation.
pub fn run<R>(
    repository: &mut R,
    client: ClientId,
    tx: TransactionId,
) -> Result<ApplyOutcome, EngineError<R::Error>>
where
    R: PaymentRepository,
    R::Error: Debug + Display,
{
    let Some(deposit) = repository.deposit(tx).map_err(repo_err)? else {
        return Ok(ApplyOutcome::Rejected(RejectionReason::DepositNotFound {
            tx,
        }));
    };
    if let Err(reason) = deposit.ensure_owned_by(client) {
        return Ok(ApplyOutcome::Rejected(reason));
    }
    let account = require_account(repository, client)?;

    match dispute_service::apply_resolve(account, deposit) {
        Ok((updated_account, _updated_deposit)) => {
            let changes = LedgerChanges::new()
                .with_account(updated_account)
                .with_deposit(DepositChange::UpdateStatus {
                    tx,
                    new_status: DepositStatus::Applied,
                });
            repository.commit(changes).map_err(repo_err)?;
            Ok(ApplyOutcome::Applied)
        }
        Err(reason) => Ok(ApplyOutcome::Rejected(reason)),
    }
}
