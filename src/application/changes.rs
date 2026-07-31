use crate::domain::{Account, Deposit, TransactionId};

/// An atomic change-set produced by a use case and handed to
/// `LedgerRepository::commit`. Only the engine constructs it; adapters
/// consume it via [`LedgerChanges::into_parts`].
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

    /// Consume the change-set into its owned parts: the updated account,
    /// an optional primary-transaction reservation, and an optional
    /// deposit upsert. External adapters use this to persist a commit
    /// however their storage requires.
    pub fn into_parts(self) -> (Account, Option<TransactionId>, Option<Deposit>) {
        (self.account, self.reserve_transaction_id, self.deposit)
    }
}
