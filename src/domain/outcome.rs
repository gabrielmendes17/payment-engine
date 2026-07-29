use crate::domain::account::ClientId;
use crate::domain::transaction::TransactionId;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApplyOutcome {
    Applied,
    Rejected(RejectionReason),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RejectionReason {
    AccountLocked {
        client: ClientId,
    },
    InvalidAmount,
    DuplicateTransaction {
        tx: TransactionId,
    },
    InsufficientFunds {
        client: ClientId,
    },
    DepositNotFound {
        tx: TransactionId,
    },
    ClientMismatch {
        tx: TransactionId,
        expected_client: ClientId,
        actual_client: ClientId,
    },
    DepositAlreadyDisputed {
        tx: TransactionId,
    },
    DepositNotDisputed {
        tx: TransactionId,
    },
    DepositAlreadyChargedBack {
        tx: TransactionId,
    },
}
