use thiserror::Error;

use crate::domain::account::ClientId;
use crate::domain::transaction::TransactionId;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum AccountError {
    #[error("invalid amount")]
    InvalidAmount,
    #[error("account {client} is locked")]
    Locked { client: ClientId },
    #[error("insufficient available funds for client {client}")]
    InsufficientFunds { client: ClientId },
    #[error("insufficient held funds for client {client}")]
    InsufficientHeldFunds { client: ClientId },
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum DepositError {
    #[error("invalid deposit amount")]
    InvalidAmount,
    #[error(
        "deposit {tx} is owned by client {owner_client}, requested by client {requesting_client}"
    )]
    ClientMismatch {
        tx: TransactionId,
        owner_client: ClientId,
        requesting_client: ClientId,
    },
    #[error("deposit {tx} is already disputed")]
    AlreadyDisputed { tx: TransactionId },
    #[error("deposit {tx} is not disputed")]
    NotDisputed { tx: TransactionId },
    #[error("deposit {tx} was already charged back")]
    AlreadyChargedBack { tx: TransactionId },
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum DisputeError {
    #[error(transparent)]
    Account(#[from] AccountError),
    #[error(transparent)]
    Deposit(#[from] DepositError),
}
