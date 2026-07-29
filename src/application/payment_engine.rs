use std::fmt::{Debug, Display};

use crate::application::errors::EngineError;
use crate::application::ports::inbound::ProcessTransaction;
use crate::application::ports::outbound::PaymentRepository;
use crate::application::use_cases::{chargeback, deposit, dispute, resolve, withdrawal};
use crate::domain::{ApplyOutcome, Transaction};

/// Thin dispatcher implementing `ProcessTransaction`. All operation logic
/// lives in the per-operation use cases and the domain layer.
pub struct PaymentEngine<R: PaymentRepository> {
    repository: R,
}

impl<R: PaymentRepository> PaymentEngine<R> {
    pub fn new(repository: R) -> Self {
        Self { repository }
    }

    pub fn repository(&self) -> &R {
        &self.repository
    }

    pub fn into_repository(self) -> R {
        self.repository
    }
}

impl<R> ProcessTransaction for PaymentEngine<R>
where
    R: PaymentRepository,
    R::Error: Debug + Display,
{
    type Error = EngineError<R::Error>;

    fn process(&mut self, transaction: Transaction) -> Result<ApplyOutcome, Self::Error> {
        match transaction {
            Transaction::Deposit { client, tx, amount } => {
                deposit::run(&mut self.repository, client, tx, amount)
            }
            Transaction::Withdrawal { client, tx, amount } => {
                withdrawal::run(&mut self.repository, client, tx, amount)
            }
            Transaction::Dispute { client, tx } => dispute::run(&mut self.repository, client, tx),
            Transaction::Resolve { client, tx } => resolve::run(&mut self.repository, client, tx),
            Transaction::Chargeback { client, tx } => {
                chargeback::run(&mut self.repository, client, tx)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    //! Orchestration tests. These exercise the full pipeline against a
    //! `FakeRepo` and verify that each dispatch path invokes the correct
    //! use case, respecting tx reservation, atomic commit shape, and the
    //! locked-account posture on primary vs lifecycle operations.
    //!
    //! Per-scenario domain rules are covered in `domain/account.rs`,
    //! `domain/deposit.rs`, and `domain/services/dispute_service.rs`.

    use super::*;
    use crate::application::changes::{AccountChange, DepositChange, LedgerChanges};
    use crate::domain::{
        Account, ClientId, DepositRecord, DepositStatus, RejectionReason, TransactionId,
    };
    use rust_decimal::Decimal;
    use rust_decimal_macros::dec;
    use std::cell::RefCell;
    use std::collections::{HashMap, HashSet};
    use std::convert::Infallible;

    #[derive(Default)]
    struct FakeRepo {
        accounts: RefCell<HashMap<ClientId, Account>>,
        seen: RefCell<HashSet<TransactionId>>,
        deposits: RefCell<HashMap<TransactionId, DepositRecord>>,
        commits: RefCell<Vec<LedgerChanges>>,
    }

    impl FakeRepo {
        fn new() -> Self {
            Self::default()
        }
        fn commit_count(&self) -> usize {
            self.commits.borrow().len()
        }
        fn last_commit(&self) -> LedgerChanges {
            self.commits
                .borrow()
                .last()
                .cloned()
                .expect("no commit recorded")
        }
    }

    impl PaymentRepository for FakeRepo {
        type Error = Infallible;

        fn transaction_seen(&self, tx: TransactionId) -> Result<bool, Self::Error> {
            Ok(self.seen.borrow().contains(&tx))
        }
        fn account(&self, client: ClientId) -> Result<Option<Account>, Self::Error> {
            Ok(self.accounts.borrow().get(&client).cloned())
        }
        fn deposit(&self, tx: TransactionId) -> Result<Option<DepositRecord>, Self::Error> {
            Ok(self.deposits.borrow().get(&tx).cloned())
        }
        fn commit(&mut self, changes: LedgerChanges) -> Result<(), Self::Error> {
            if let Some(AccountChange::Upsert(account)) = &changes.account {
                self.accounts
                    .borrow_mut()
                    .insert(account.client_id(), account.clone());
            }
            if let Some(tx) = changes.reserve_transaction_id {
                self.seen.borrow_mut().insert(tx);
            }
            match &changes.deposit {
                Some(DepositChange::Insert(deposit)) => {
                    self.deposits
                        .borrow_mut()
                        .insert(deposit.transaction_id(), deposit.clone());
                }
                Some(DepositChange::UpdateStatus { tx, new_status }) => {
                    if let Some(record) = self.deposits.borrow_mut().get_mut(tx) {
                        // We can't set the field directly (private), so
                        // re-insert. Domain provides the transitions.
                        let updated = match new_status {
                            DepositStatus::Disputed => record.clone().begin_dispute().unwrap(),
                            DepositStatus::Applied => record.clone().resolve().unwrap(),
                            DepositStatus::ChargedBack => record.clone().charge_back().unwrap(),
                        };
                        *record = updated;
                    }
                }
                None => {}
            }
            self.commits.borrow_mut().push(changes);
            Ok(())
        }
        fn accounts(&self) -> Result<Vec<Account>, Self::Error> {
            Ok(self.accounts.borrow().values().cloned().collect())
        }
    }

    fn engine() -> PaymentEngine<FakeRepo> {
        PaymentEngine::new(FakeRepo::new())
    }

    // Dispatch: deposit path applies and commits a full ledger change
    #[test]
    fn deposit_dispatches_and_commits_one_atomic_change_set() {
        let mut e = engine();
        let out = e
            .process(Transaction::Deposit {
                client: 1,
                tx: 1,
                amount: dec!(10.0000),
            })
            .unwrap();
        assert_eq!(out, ApplyOutcome::Applied);
        assert_eq!(e.repository().commit_count(), 1);
        let c = e.repository().last_commit();
        assert!(c.account.is_some());
        assert_eq!(c.reserve_transaction_id, Some(1));
        assert!(matches!(c.deposit, Some(DepositChange::Insert(_))));
    }

    // Dispatch: withdrawal path applies and does not insert a deposit
    #[test]
    fn withdrawal_dispatches_and_commits_without_deposit_record() {
        let mut e = engine();
        e.process(Transaction::Deposit {
            client: 1,
            tx: 1,
            amount: dec!(10.0000),
        })
        .unwrap();
        e.process(Transaction::Withdrawal {
            client: 1,
            tx: 2,
            amount: dec!(3.0000),
        })
        .unwrap();
        let c = e.repository().last_commit();
        assert!(c.account.is_some());
        assert_eq!(c.reserve_transaction_id, Some(2));
        assert!(c.deposit.is_none());
    }

    // Duplicate primary tx rejected without a new commit
    #[test]
    fn duplicate_primary_tx_is_rejected_and_does_not_commit_again() {
        let mut e = engine();
        e.process(Transaction::Deposit {
            client: 1,
            tx: 1,
            amount: dec!(10.0000),
        })
        .unwrap();
        let before = e.repository().commit_count();
        let out = e
            .process(Transaction::Deposit {
                client: 1,
                tx: 1,
                amount: dec!(1),
            })
            .unwrap();
        assert_eq!(
            out,
            ApplyOutcome::Rejected(RejectionReason::DuplicateTransaction { tx: 1 })
        );
        assert_eq!(e.repository().commit_count(), before);
    }

    // Rejected primary tx (invalid amount / locked / insufficient) still reserves tx
    #[test]
    fn rejected_primary_tx_reserves_tx() {
        let mut e = engine();
        let out = e
            .process(Transaction::Deposit {
                client: 1,
                tx: 1,
                amount: Decimal::ZERO,
            })
            .unwrap();
        assert_eq!(out, ApplyOutcome::Rejected(RejectionReason::InvalidAmount));
        assert!(e.repository().transaction_seen(1).unwrap());
        assert_eq!(e.repository().last_commit().reserve_transaction_id, Some(1));
        assert!(e.repository().last_commit().account.is_none());
    }

    // Locked account rejects new primary activity and reserves tx (scenario 13)
    #[test]
    fn locked_account_rejects_primary_activity_and_reserves_tx() {
        let mut e = engine();
        e.process(Transaction::Deposit {
            client: 1,
            tx: 1,
            amount: dec!(10.0000),
        })
        .unwrap();
        e.process(Transaction::Dispute { client: 1, tx: 1 })
            .unwrap();
        e.process(Transaction::Chargeback { client: 1, tx: 1 })
            .unwrap();
        assert!(e.repository().account(1).unwrap().unwrap().is_locked());

        let out = e
            .process(Transaction::Deposit {
                client: 1,
                tx: 99,
                amount: dec!(5),
            })
            .unwrap();
        assert_eq!(
            out,
            ApplyOutcome::Rejected(RejectionReason::AccountLocked { client: 1 })
        );
        assert!(e.repository().transaction_seen(99).unwrap());
    }

    // Rejected lifecycle events do not commit
    #[test]
    fn rejected_lifecycle_events_do_not_commit() {
        let mut e = engine();
        e.process(Transaction::Deposit {
            client: 1,
            tx: 1,
            amount: dec!(10.0000),
        })
        .unwrap();
        let before = e.repository().commit_count();

        // Resolve before dispute → rejected, no commit
        e.process(Transaction::Resolve { client: 1, tx: 1 })
            .unwrap();
        // Chargeback before dispute → rejected, no commit
        e.process(Transaction::Chargeback { client: 1, tx: 1 })
            .unwrap();
        // Cross-client dispute → rejected, no commit
        e.process(Transaction::Dispute { client: 2, tx: 1 })
            .unwrap();
        // Unknown deposit → rejected, no commit
        e.process(Transaction::Dispute { client: 1, tx: 999 })
            .unwrap();

        assert_eq!(e.repository().commit_count(), before);
    }

    // End-to-end pipeline: dispute + chargeback locks account and preserves total
    // invariants (scenario 12 shape: dispute may leave available negative)
    #[test]
    fn dispute_after_withdrawal_leaves_available_negative_then_chargeback_locks() {
        let mut e = engine();
        e.process(Transaction::Deposit {
            client: 1,
            tx: 1,
            amount: dec!(10.0000),
        })
        .unwrap();
        e.process(Transaction::Withdrawal {
            client: 1,
            tx: 2,
            amount: dec!(7.0000),
        })
        .unwrap();
        e.process(Transaction::Dispute { client: 1, tx: 1 })
            .unwrap();
        let acc = e.repository().account(1).unwrap().unwrap();
        assert_eq!(acc.available(), dec!(-7.0000));
        assert_eq!(acc.held(), dec!(10.0000));
        assert_eq!(acc.total(), dec!(3.0000));

        e.process(Transaction::Chargeback { client: 1, tx: 1 })
            .unwrap();
        let acc = e.repository().account(1).unwrap().unwrap();
        assert_eq!(acc.available(), dec!(-7.0000));
        assert_eq!(acc.held(), Decimal::ZERO);
        assert_eq!(acc.total(), dec!(-7.0000));
        assert!(acc.is_locked());
    }
}
