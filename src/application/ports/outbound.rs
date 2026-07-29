use crate::application::changes::LedgerChanges;
use crate::domain::{Account, ClientId, DepositRecord, TransactionId};

pub trait PaymentRepository {
    type Error;

    fn transaction_seen(&self, tx: TransactionId) -> Result<bool, Self::Error>;

    fn account(&self, client: ClientId) -> Result<Option<Account>, Self::Error>;

    fn deposit(&self, tx: TransactionId) -> Result<Option<DepositRecord>, Self::Error>;

    fn commit(&mut self, changes: LedgerChanges) -> Result<(), Self::Error>;

    fn accounts(&self) -> Result<Vec<Account>, Self::Error>;
}
