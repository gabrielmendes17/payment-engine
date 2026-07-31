use crate::domain::errors::DepositError;
use crate::domain::{ClientId, TransactionId};

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
    InsufficientHeldFunds {
        client: ClientId,
    },
    DepositNotFound {
        tx: TransactionId,
    },
    ClientMismatch {
        tx: TransactionId,
        owner_client: ClientId,
        requesting_client: ClientId,
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

// No blanket `From<AccountError>` / `From<DisputeError>`:
// `ArithmeticOverflow` must terminate processing rather than downgrade to
// a rejection. Callers go through `helpers::classify_account_error` /
// `helpers::classify_dispute_error`.
impl From<DepositError> for RejectionReason {
    fn from(e: DepositError) -> Self {
        match e {
            DepositError::InvalidAmount => Self::InvalidAmount,
            DepositError::ClientMismatch {
                tx,
                owner_client,
                requesting_client,
            } => Self::ClientMismatch {
                tx,
                owner_client,
                requesting_client,
            },
            DepositError::AlreadyDisputed { tx } => Self::DepositAlreadyDisputed { tx },
            DepositError::NotDisputed { tx } => Self::DepositNotDisputed { tx },
            DepositError::AlreadyChargedBack { tx } => Self::DepositAlreadyChargedBack { tx },
        }
    }
}
