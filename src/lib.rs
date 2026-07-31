//! Payment engine library.
//!
//! Outbound persistence is a public port (`LedgerRepository`). External
//! adapters consume a committed change-set via [`LedgerChanges::into_parts`].

pub mod adapters;
mod application;
pub mod domain;

pub use application::{
    ApplyOutcome, EngineError, LedgerChanges, LedgerRepository, ListAccounts, PaymentEngine,
    ProcessTransaction, RejectionReason,
};
