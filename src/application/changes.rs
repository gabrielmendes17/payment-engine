use crate::domain::{Account, DepositRecord, DepositStatus, TransactionId};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AccountChange {
    Upsert(Account),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DepositChange {
    Insert(DepositRecord),
    UpdateStatus {
        tx: TransactionId,
        new_status: DepositStatus,
    },
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LedgerChanges {
    pub account: Option<AccountChange>,
    pub reserve_transaction_id: Option<TransactionId>,
    pub deposit: Option<DepositChange>,
}

impl LedgerChanges {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_account(mut self, account: Account) -> Self {
        self.account = Some(AccountChange::Upsert(account));
        self
    }

    pub fn reserving(mut self, tx: TransactionId) -> Self {
        self.reserve_transaction_id = Some(tx);
        self
    }

    pub fn with_deposit(mut self, change: DepositChange) -> Self {
        self.deposit = Some(change);
        self
    }
}
