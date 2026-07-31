use crate::domain::{Account, Deposit, TransactionId};

/// Fields are `pub(crate)` so external code cannot build or inspect a
/// change-set — this effectively seals `LedgerRepository`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LedgerChanges {
    pub(crate) account: Account,
    pub(crate) reserve_transaction_id: Option<TransactionId>,
    pub(crate) deposit: Option<Deposit>,
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
}
