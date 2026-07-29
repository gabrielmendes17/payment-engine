use crate::domain::{ApplyOutcome, Transaction};

pub trait ProcessTransaction {
    type Error;

    fn process(&mut self, transaction: Transaction) -> Result<ApplyOutcome, Self::Error>;
}
