use crate::domain::errors::{AccountError, DepositError, DisputeError};
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

impl From<AccountError> for RejectionReason {
    fn from(e: AccountError) -> Self {
        match e {
            AccountError::InvalidAmount => Self::InvalidAmount,
            AccountError::Locked { client } => Self::AccountLocked { client },
            AccountError::InsufficientFunds { client } => Self::InsufficientFunds { client },
            AccountError::InsufficientHeldFunds { client } => {
                Self::InsufficientHeldFunds { client }
            }
        }
    }
}

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

impl From<DisputeError> for RejectionReason {
    fn from(e: DisputeError) -> Self {
        match e {
            DisputeError::Account(a) => a.into(),
            DisputeError::Deposit(d) => d.into(),
        }
    }
}
