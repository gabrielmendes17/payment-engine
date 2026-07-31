//! Payment engine library.
//!
//! The outbound `LedgerRepository` port is effectively sealed: `LedgerChanges`
//! has `pub(crate)` fields, so any additional repository adapter should live
//! inside this crate.

pub mod adapters;
mod application;
pub mod domain;

pub use application::{
    ApplyOutcome, EngineError, ListAccounts, PaymentEngine, ProcessTransaction, RejectionReason,
};
