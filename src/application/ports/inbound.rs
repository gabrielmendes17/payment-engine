use crate::application::outcome::ApplyOutcome;
use crate::domain::{Account, Transaction};

pub trait ProcessTransaction {
    type Error: std::error::Error + Send + Sync + 'static;

    fn process(&mut self, transaction: Transaction) -> Result<ApplyOutcome, Self::Error>;
}

pub trait ListAccounts {
    type Error: std::error::Error + Send + Sync + 'static;

    fn list_accounts(&self) -> Result<Vec<Account>, Self::Error>;
}
