use crate::application::errors::EngineError;
use crate::application::outcome::ApplyOutcome;
use crate::application::ports::inbound::{ListAccounts, ProcessTransaction};
use crate::application::ports::outbound::LedgerRepository;
use crate::application::use_cases::{chargeback, deposit, dispute, resolve, withdrawal};
use crate::domain::{Account, Transaction};

pub struct PaymentEngine<R: LedgerRepository> {
    repository: R,
}

impl<R: LedgerRepository> PaymentEngine<R> {
    pub fn new(repository: R) -> Self {
        Self { repository }
    }
}

impl<R> ProcessTransaction for PaymentEngine<R>
where
    R: LedgerRepository,
{
    type Error = EngineError<R::Error>;

    fn process(&mut self, transaction: Transaction) -> Result<ApplyOutcome, Self::Error> {
        match transaction {
            Transaction::Deposit { client, tx, amount } => {
                deposit::execute(&mut self.repository, client, tx, amount)
            }
            Transaction::Withdrawal { client, tx, amount } => {
                withdrawal::execute(&mut self.repository, client, tx, amount)
            }
            Transaction::Dispute { client, tx } => {
                dispute::execute(&mut self.repository, client, tx)
            }
            Transaction::Resolve { client, tx } => {
                resolve::execute(&mut self.repository, client, tx)
            }
            Transaction::Chargeback { client, tx } => {
                chargeback::execute(&mut self.repository, client, tx)
            }
        }
    }
}

impl<R> ListAccounts for PaymentEngine<R>
where
    R: LedgerRepository,
{
    type Error = EngineError<R::Error>;

    fn list_accounts(&self) -> Result<Vec<Account>, Self::Error> {
        self.repository.accounts().map_err(EngineError::Repository)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::changes::LedgerChanges;
    use crate::application::outcome::{ApplyOutcome, RejectionReason};
    use crate::domain::{Account, ClientId, Deposit, DepositStatus, Transaction, TransactionId};
    use rust_decimal::Decimal;
    use rust_decimal_macros::dec;
    use std::cell::RefCell;
    use std::collections::{HashMap, HashSet};
    use std::convert::Infallible;
    use std::rc::Rc;

    #[derive(Default, Clone)]
    struct FakeState {
        accounts: Rc<RefCell<HashMap<ClientId, Account>>>,
        seen: Rc<RefCell<HashSet<TransactionId>>>,
        deposits: Rc<RefCell<HashMap<TransactionId, Deposit>>>,
        commits: Rc<RefCell<Vec<LedgerChanges>>>,
    }

    impl FakeState {
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
        fn account(&self, client: ClientId) -> Option<Account> {
            self.accounts.borrow().get(&client).cloned()
        }
        fn transaction_seen(&self, tx: TransactionId) -> bool {
            self.seen.borrow().contains(&tx)
        }
        fn deposit(&self, tx: TransactionId) -> Option<Deposit> {
            self.deposits.borrow().get(&tx).cloned()
        }
        fn seed_account(&self, account: Account) {
            self.accounts
                .borrow_mut()
                .insert(account.client_id(), account);
        }
    }

    struct FakeRepo {
        state: FakeState,
    }

    impl FakeRepo {
        fn new() -> Self {
            Self {
                state: FakeState::default(),
            }
        }
        fn state(&self) -> FakeState {
            self.state.clone()
        }
    }

    impl LedgerRepository for FakeRepo {
        type Error = Infallible;

        fn transaction_seen(&self, tx: TransactionId) -> Result<bool, Self::Error> {
            Ok(self.state.seen.borrow().contains(&tx))
        }
        fn account(&self, client: ClientId) -> Result<Option<Account>, Self::Error> {
            Ok(self.state.accounts.borrow().get(&client).cloned())
        }
        fn deposit(&self, tx: TransactionId) -> Result<Option<Deposit>, Self::Error> {
            Ok(self.state.deposits.borrow().get(&tx).cloned())
        }
        fn commit(&mut self, changes: LedgerChanges) -> Result<(), Self::Error> {
            let account = &changes.account;
            self.state
                .accounts
                .borrow_mut()
                .insert(account.client_id(), account.clone());
            if let Some(tx) = changes.reserve_transaction_id {
                self.state.seen.borrow_mut().insert(tx);
            }
            if let Some(deposit) = &changes.deposit {
                self.state
                    .deposits
                    .borrow_mut()
                    .insert(deposit.transaction_id(), deposit.clone());
            }
            self.state.commits.borrow_mut().push(changes);
            Ok(())
        }
        fn accounts(&self) -> Result<Vec<Account>, Self::Error> {
            Ok(self.state.accounts.borrow().values().cloned().collect())
        }
    }

    fn engine() -> (PaymentEngine<FakeRepo>, FakeState) {
        let repo = FakeRepo::new();
        let state = repo.state();
        (PaymentEngine::new(repo), state)
    }

    #[test]
    fn deposit_dispatches_and_commits_one_atomic_change_set() {
        let (mut e, state) = engine();
        let out = e
            .process(Transaction::Deposit {
                client: 1,
                tx: 1,
                amount: dec!(10.0000),
            })
            .unwrap();
        assert_eq!(out, ApplyOutcome::Applied);
        assert_eq!(state.commit_count(), 1);
        let c = state.last_commit();
        assert_eq!(c.account.client_id(), 1);
        assert_eq!(c.reserve_transaction_id, Some(1));
        assert_eq!(c.deposit.map(|d| d.status()), Some(DepositStatus::Applied));
    }

    #[test]
    fn withdrawal_dispatches_and_commits_without_deposit_record() {
        let (mut e, state) = engine();
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
        let c = state.last_commit();
        assert_eq!(c.account.client_id(), 1);
        assert_eq!(c.reserve_transaction_id, Some(2));
        assert!(c.deposit.is_none());
    }

    #[test]
    fn duplicate_primary_tx_is_rejected_and_does_not_commit_again() {
        let (mut e, state) = engine();
        e.process(Transaction::Deposit {
            client: 1,
            tx: 1,
            amount: dec!(10.0000),
        })
        .unwrap();
        let before = state.commit_count();
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
        assert_eq!(state.commit_count(), before);
    }

    #[test]
    fn rejected_primary_tx_reserves_tx_and_persists_account() {
        let (mut e, state) = engine();
        let out = e
            .process(Transaction::Deposit {
                client: 1,
                tx: 1,
                amount: Decimal::ZERO,
            })
            .unwrap();
        assert_eq!(out, ApplyOutcome::Rejected(RejectionReason::InvalidAmount));
        assert!(state.transaction_seen(1));
        assert_eq!(state.last_commit().reserve_transaction_id, Some(1));

        let account = state.account(1).expect("account persisted");
        assert_eq!(account.available(), Decimal::ZERO);
        assert_eq!(account.held(), Decimal::ZERO);
        assert!(!account.is_locked());
    }

    #[test]
    fn rejected_first_time_withdrawal_leaves_client_ready_for_next_tx() {
        let (mut e, state) = engine();
        let out = e
            .process(Transaction::Withdrawal {
                client: 7,
                tx: 1,
                amount: dec!(5),
            })
            .unwrap();
        assert_eq!(
            out,
            ApplyOutcome::Rejected(RejectionReason::InsufficientFunds { client: 7 })
        );
        assert!(
            state.account(7).is_some(),
            "client account must be persisted"
        );

        e.process(Transaction::Deposit {
            client: 7,
            tx: 2,
            amount: dec!(3),
        })
        .unwrap();
        assert_eq!(state.account(7).unwrap().available(), dec!(3));
    }

    #[test]
    fn locked_account_rejects_primary_activity_and_reserves_tx() {
        let (mut e, state) = engine();
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
        assert!(state.account(1).unwrap().is_locked());

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
        assert!(state.transaction_seen(99));
    }

    #[test]
    fn rejected_lifecycle_events_do_not_commit() {
        let (mut e, state) = engine();
        e.process(Transaction::Deposit {
            client: 1,
            tx: 1,
            amount: dec!(10.0000),
        })
        .unwrap();
        let before = state.commit_count();

        e.process(Transaction::Resolve { client: 1, tx: 1 })
            .unwrap();
        e.process(Transaction::Chargeback { client: 1, tx: 1 })
            .unwrap();
        e.process(Transaction::Dispute { client: 2, tx: 1 })
            .unwrap();
        e.process(Transaction::Dispute { client: 1, tx: 999 })
            .unwrap();

        assert_eq!(state.commit_count(), before);
    }

    #[test]
    fn dispute_after_withdrawal_leaves_available_negative_then_chargeback_locks() {
        let (mut e, state) = engine();
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
        let acc = state.account(1).unwrap();
        assert_eq!(acc.available(), dec!(-7.0000));
        assert_eq!(acc.held(), dec!(10.0000));
        assert_eq!(acc.total(), dec!(3.0000));

        e.process(Transaction::Chargeback { client: 1, tx: 1 })
            .unwrap();
        let acc = state.account(1).unwrap();
        assert_eq!(acc.available(), dec!(-7.0000));
        assert_eq!(acc.held(), Decimal::ZERO);
        assert_eq!(acc.total(), dec!(-7.0000));
        assert!(acc.is_locked());
    }

    #[test]
    fn dispute_upserts_deposit_with_disputed_status() {
        let (mut e, state) = engine();
        e.process(Transaction::Deposit {
            client: 1,
            tx: 1,
            amount: dec!(10.0000),
        })
        .unwrap();
        e.process(Transaction::Dispute { client: 1, tx: 1 })
            .unwrap();

        let d = state.deposit(1).expect("deposit exists");
        assert_eq!(d.status(), DepositStatus::Disputed);
    }

    #[test]
    fn frozen_account_rejects_chargeback_of_another_disputed_deposit() {
        let (mut e, state) = engine();
        for (tx, amount) in [(1u32, dec!(10.0000)), (2, dec!(5.0000))] {
            e.process(Transaction::Deposit {
                client: 1,
                tx,
                amount,
            })
            .unwrap();
        }
        e.process(Transaction::Dispute { client: 1, tx: 1 })
            .unwrap();
        e.process(Transaction::Dispute { client: 1, tx: 2 })
            .unwrap();
        e.process(Transaction::Chargeback { client: 1, tx: 1 })
            .unwrap();
        assert!(state.account(1).unwrap().is_locked());

        let out = e
            .process(Transaction::Chargeback { client: 1, tx: 2 })
            .unwrap();
        assert_eq!(
            out,
            ApplyOutcome::Rejected(RejectionReason::AccountLocked { client: 1 })
        );
        assert_eq!(state.deposit(2).unwrap().status(), DepositStatus::Disputed);
    }

    #[test]
    fn cross_client_dispute_does_not_create_phantom_account() {
        let (mut e, state) = engine();
        e.process(Transaction::Deposit {
            client: 1,
            tx: 1,
            amount: dec!(10.0000),
        })
        .unwrap();

        let out = e
            .process(Transaction::Dispute { client: 2, tx: 1 })
            .unwrap();
        assert_eq!(
            out,
            ApplyOutcome::Rejected(RejectionReason::ClientMismatch {
                tx: 1,
                owner_client: 1,
                requesting_client: 2,
            })
        );
        assert!(state.account(2).is_none());
    }

    #[test]
    fn frozen_account_rejects_dispute_of_a_different_applied_deposit() {
        let (mut e, state) = engine();
        for (tx, amount) in [(1u32, dec!(10.0000)), (2, dec!(5.0000))] {
            e.process(Transaction::Deposit {
                client: 1,
                tx,
                amount,
            })
            .unwrap();
        }
        e.process(Transaction::Dispute { client: 1, tx: 1 })
            .unwrap();
        e.process(Transaction::Chargeback { client: 1, tx: 1 })
            .unwrap();

        let out = e
            .process(Transaction::Dispute { client: 1, tx: 2 })
            .unwrap();
        assert_eq!(
            out,
            ApplyOutcome::Rejected(RejectionReason::AccountLocked { client: 1 })
        );
        assert_eq!(state.deposit(2).unwrap().status(), DepositStatus::Applied);
    }

    #[test]
    fn frozen_account_rejects_resolve_of_a_previously_disputed_deposit() {
        let (mut e, state) = engine();
        for (tx, amount) in [(1u32, dec!(10.0000)), (2, dec!(5.0000))] {
            e.process(Transaction::Deposit {
                client: 1,
                tx,
                amount,
            })
            .unwrap();
        }
        e.process(Transaction::Dispute { client: 1, tx: 2 })
            .unwrap();
        e.process(Transaction::Dispute { client: 1, tx: 1 })
            .unwrap();
        e.process(Transaction::Chargeback { client: 1, tx: 1 })
            .unwrap();

        let out = e
            .process(Transaction::Resolve { client: 1, tx: 2 })
            .unwrap();
        assert_eq!(
            out,
            ApplyOutcome::Rejected(RejectionReason::AccountLocked { client: 1 })
        );
        assert_eq!(state.deposit(2).unwrap().status(), DepositStatus::Disputed);
    }

    #[test]
    fn multiple_simultaneous_disputes_hold_independently() {
        let (mut e, state) = engine();
        e.process(Transaction::Deposit {
            client: 1,
            tx: 1,
            amount: dec!(7.0000),
        })
        .unwrap();
        e.process(Transaction::Deposit {
            client: 1,
            tx: 2,
            amount: dec!(3.0000),
        })
        .unwrap();
        e.process(Transaction::Dispute { client: 1, tx: 1 })
            .unwrap();
        e.process(Transaction::Dispute { client: 1, tx: 2 })
            .unwrap();

        let acc = state.account(1).unwrap();
        assert_eq!(acc.available(), Decimal::ZERO);
        assert_eq!(acc.held(), dec!(10.0000));
        assert_eq!(acc.total(), dec!(10.0000));

        e.process(Transaction::Resolve { client: 1, tx: 1 })
            .unwrap();
        let acc = state.account(1).unwrap();
        assert_eq!(acc.available(), dec!(7.0000));
        assert_eq!(acc.held(), dec!(3.0000));
        assert_eq!(acc.total(), dec!(10.0000));
        assert!(!acc.is_locked());
    }

    #[test]
    fn lifecycle_events_on_a_withdrawal_tx_all_reject_deposit_not_found() {
        let (mut e, _state) = engine();
        e.process(Transaction::Deposit {
            client: 1,
            tx: 1,
            amount: dec!(10.0000),
        })
        .unwrap();
        e.process(Transaction::Withdrawal {
            client: 1,
            tx: 2,
            amount: dec!(4.0000),
        })
        .unwrap();

        for op in [
            Transaction::Dispute { client: 1, tx: 2 },
            Transaction::Resolve { client: 1, tx: 2 },
            Transaction::Chargeback { client: 1, tx: 2 },
        ] {
            assert_eq!(
                e.process(op).unwrap(),
                ApplyOutcome::Rejected(RejectionReason::DepositNotFound { tx: 2 })
            );
        }
    }

    #[test]
    fn wrong_client_on_resolve_and_chargeback_rejects_without_touching_state() {
        let (mut e, state) = engine();
        e.process(Transaction::Deposit {
            client: 1,
            tx: 1,
            amount: dec!(10.0000),
        })
        .unwrap();
        e.process(Transaction::Dispute { client: 1, tx: 1 })
            .unwrap();
        let commits_before = state.commit_count();

        let mismatch = RejectionReason::ClientMismatch {
            tx: 1,
            owner_client: 1,
            requesting_client: 2,
        };
        assert_eq!(
            e.process(Transaction::Resolve { client: 2, tx: 1 })
                .unwrap(),
            ApplyOutcome::Rejected(mismatch.clone())
        );
        assert_eq!(
            e.process(Transaction::Chargeback { client: 2, tx: 1 })
                .unwrap(),
            ApplyOutcome::Rejected(mismatch)
        );

        assert_eq!(state.commit_count(), commits_before);
        assert!(state.account(2).is_none());
        assert_eq!(state.deposit(1).unwrap().status(), DepositStatus::Disputed);
    }

    #[test]
    fn deposit_overflow_returns_engine_error_and_does_not_commit() {
        let (mut e, state) = engine();
        let seeded = Account::new(1)
            .credit(Decimal::MAX)
            .unwrap()
            .hold(Decimal::MAX)
            .unwrap();
        state.seed_account(seeded);
        let commits_before = state.commit_count();

        let err = e
            .process(Transaction::Deposit {
                client: 1,
                tx: 99,
                amount: dec!(1),
            })
            .unwrap_err();
        assert!(matches!(err, EngineError::ArithmeticOverflow { client: 1 }));
        assert_eq!(state.commit_count(), commits_before);
        let account = state.account(1).unwrap();
        assert_eq!(account.available(), Decimal::ZERO);
        assert_eq!(account.held(), Decimal::MAX);
        assert!(!state.transaction_seen(99));
    }
}
