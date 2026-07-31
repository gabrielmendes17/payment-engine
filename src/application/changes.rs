use crate::domain::{Account, Deposit, TransactionId};

/// An atomic change-set produced by a use case and handed to
/// `LedgerRepository::commit`. Only use cases inside this crate
/// construct it; adapters consume it via [`LedgerChanges::into_parts`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LedgerChanges {
    pub(crate) account: Account,
    pub(crate) reserve_transaction_id: Option<TransactionId>,
    pub(crate) deposit: Option<Deposit>,
}

/// Owned view of a committed [`LedgerChanges`] for adapters to persist.
///
/// - `account` — the updated account (always present).
/// - `reserve_transaction_id` — a primary-transaction id to reserve
///   against reuse. `None` for lifecycle-only commits
///   (dispute/resolve/chargeback).
/// - `deposit` — a deposit upsert (insert for a new deposit, update
///   for a lifecycle transition). `None` for withdrawals.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommittedChanges {
    pub account: Account,
    pub reserve_transaction_id: Option<TransactionId>,
    pub deposit: Option<Deposit>,
}

impl LedgerChanges {
    pub(crate) fn new(account: Account) -> Self {
        Self {
            account,
            reserve_transaction_id: None,
            deposit: None,
        }
    }

    pub(crate) fn reserving(mut self, tx: TransactionId) -> Self {
        self.reserve_transaction_id = Some(tx);
        self
    }

    pub(crate) fn with_deposit(mut self, deposit: Deposit) -> Self {
        self.deposit = Some(deposit);
        self
    }

    /// Consume the change-set into a [`CommittedChanges`] view. External
    /// adapters use this to persist a commit however their storage
    /// requires.
    pub fn into_parts(self) -> CommittedChanges {
        CommittedChanges {
            account: self.account,
            reserve_transaction_id: self.reserve_transaction_id,
            deposit: self.deposit,
        }
    }
}
