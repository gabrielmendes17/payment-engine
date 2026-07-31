use crate::application::changes::LedgerChanges;
use crate::domain::{Account, ClientId, Deposit, TransactionId};

/// Effectively sealed: `LedgerChanges` has `pub(crate)` fields, so a
/// foreign implementor could not inspect the change-set it receives.
pub trait LedgerRepository {
    type Error: std::error::Error + Send + Sync + 'static;

    fn transaction_seen(&self, tx: TransactionId) -> Result<bool, Self::Error>;

    fn account(&self, client: ClientId) -> Result<Option<Account>, Self::Error>;

    fn deposit(&self, tx: TransactionId) -> Result<Option<Deposit>, Self::Error>;

    fn commit(&mut self, changes: LedgerChanges) -> Result<(), Self::Error>;

    fn accounts(&self) -> Result<Vec<Account>, Self::Error>;
}
