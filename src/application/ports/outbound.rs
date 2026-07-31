use crate::application::changes::LedgerChanges;
use crate::domain::{Account, ClientId, Deposit, TransactionId};

/// Outbound port implemented by ledger adapters.
///
/// Adapters back the four read operations (`transaction_seen`,
/// `account`, `deposit`, `accounts`) with their storage of choice and
/// persist a committed change-set by destructuring it via
/// [`LedgerChanges::into_parts`]. See `InMemoryLedgerRepository` for the
/// reference in-memory adapter.
pub trait LedgerRepository {
    type Error: std::error::Error + Send + Sync + 'static;

    fn transaction_seen(&self, tx: TransactionId) -> Result<bool, Self::Error>;

    fn account(&self, client: ClientId) -> Result<Option<Account>, Self::Error>;

    fn deposit(&self, tx: TransactionId) -> Result<Option<Deposit>, Self::Error>;

    fn commit(&mut self, changes: LedgerChanges) -> Result<(), Self::Error>;

    fn accounts(&self) -> Result<Vec<Account>, Self::Error>;
}
