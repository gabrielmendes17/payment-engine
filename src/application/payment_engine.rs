use std::fmt::{Debug, Display};

use rust_decimal::Decimal;

use crate::application::changes::{DepositChange, LedgerChanges};
use crate::application::errors::EngineError;
use crate::application::ports::inbound::ProcessTransaction;
use crate::application::ports::outbound::PaymentRepository;
use crate::domain::{
    Account, ApplyOutcome, ClientId, DepositRecord, DepositStatus, RejectionReason, Transaction,
    TransactionId,
};

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
            Transaction::Deposit { client, tx, amount } => self.deposit(client, tx, amount),
            Transaction::Withdrawal { client, tx, amount } => self.withdrawal(client, tx, amount),
            Transaction::Dispute { client, tx } => self.dispute(client, tx),
            Transaction::Resolve { client, tx } => self.resolve(client, tx),
            Transaction::Chargeback { client, tx } => self.chargeback(client, tx),
        }
    }
}

impl<R> PaymentEngine<R>
where
    R: PaymentRepository,
    R::Error: Debug + Display,
{
    fn deposit(
        &mut self,
        client: ClientId,
        tx: TransactionId,
        amount: Decimal,
    ) -> Result<ApplyOutcome, EngineError<R::Error>> {
        if self.repository.transaction_seen(tx).map_err(repo)? {
            return Ok(ApplyOutcome::Rejected(
                RejectionReason::DuplicateTransaction { tx },
            ));
        }

        if amount <= Decimal::ZERO {
            let changes = LedgerChanges::new().reserving(tx);
            self.repository.commit(changes).map_err(repo)?;
            return Ok(ApplyOutcome::Rejected(RejectionReason::InvalidAmount));
        }

        let account = self.load_or_new_account(client)?;
        if account.locked {
            let changes = LedgerChanges::new().reserving(tx);
            self.repository.commit(changes).map_err(repo)?;
            return Ok(ApplyOutcome::Rejected(RejectionReason::AccountLocked {
                client,
            }));
        }

        let mut updated = account;
        updated.available += amount;
        let deposit = DepositRecord::new_applied(tx, client, amount);

        let changes = LedgerChanges::new()
            .reserving(tx)
            .with_account(updated)
            .with_deposit(DepositChange::Insert(deposit));
        self.repository.commit(changes).map_err(repo)?;
        Ok(ApplyOutcome::Applied)
    }

    fn withdrawal(
        &mut self,
        client: ClientId,
        tx: TransactionId,
        amount: Decimal,
    ) -> Result<ApplyOutcome, EngineError<R::Error>> {
        if self.repository.transaction_seen(tx).map_err(repo)? {
            return Ok(ApplyOutcome::Rejected(
                RejectionReason::DuplicateTransaction { tx },
            ));
        }

        if amount <= Decimal::ZERO {
            let changes = LedgerChanges::new().reserving(tx);
            self.repository.commit(changes).map_err(repo)?;
            return Ok(ApplyOutcome::Rejected(RejectionReason::InvalidAmount));
        }

        let account = self.load_or_new_account(client)?;
        if account.locked {
            let changes = LedgerChanges::new().reserving(tx);
            self.repository.commit(changes).map_err(repo)?;
            return Ok(ApplyOutcome::Rejected(RejectionReason::AccountLocked {
                client,
            }));
        }
        if account.available < amount {
            let changes = LedgerChanges::new().reserving(tx);
            self.repository.commit(changes).map_err(repo)?;
            return Ok(ApplyOutcome::Rejected(RejectionReason::InsufficientFunds {
                client,
            }));
        }

        let mut updated = account;
        updated.available -= amount;
        let changes = LedgerChanges::new().reserving(tx).with_account(updated);
        self.repository.commit(changes).map_err(repo)?;
        Ok(ApplyOutcome::Applied)
    }

    fn dispute(
        &mut self,
        client: ClientId,
        tx: TransactionId,
    ) -> Result<ApplyOutcome, EngineError<R::Error>> {
        let Some(deposit) = self.repository.deposit(tx).map_err(repo)? else {
            return Ok(ApplyOutcome::Rejected(RejectionReason::DepositNotFound {
                tx,
            }));
        };
        if deposit.client_id != client {
            return Ok(ApplyOutcome::Rejected(RejectionReason::ClientMismatch {
                tx,
                expected_client: deposit.client_id,
                actual_client: client,
            }));
        }
        let account = self.require_account(client)?;
        if account.locked {
            return Ok(ApplyOutcome::Rejected(RejectionReason::AccountLocked {
                client,
            }));
        }
        match deposit.status {
            DepositStatus::Disputed => {
                return Ok(ApplyOutcome::Rejected(
                    RejectionReason::DepositAlreadyDisputed { tx },
                ));
            }
            DepositStatus::ChargedBack => {
                return Ok(ApplyOutcome::Rejected(
                    RejectionReason::DepositAlreadyChargedBack { tx },
                ));
            }
            DepositStatus::Applied => {}
        }

        let mut updated = account;
        updated.available -= deposit.amount;
        updated.held += deposit.amount;
        let changes =
            LedgerChanges::new()
                .with_account(updated)
                .with_deposit(DepositChange::UpdateStatus {
                    tx,
                    new_status: DepositStatus::Disputed,
                });
        self.repository.commit(changes).map_err(repo)?;
        Ok(ApplyOutcome::Applied)
    }

    fn resolve(
        &mut self,
        client: ClientId,
        tx: TransactionId,
    ) -> Result<ApplyOutcome, EngineError<R::Error>> {
        let Some(deposit) = self.repository.deposit(tx).map_err(repo)? else {
            return Ok(ApplyOutcome::Rejected(RejectionReason::DepositNotFound {
                tx,
            }));
        };
        if deposit.client_id != client {
            return Ok(ApplyOutcome::Rejected(RejectionReason::ClientMismatch {
                tx,
                expected_client: deposit.client_id,
                actual_client: client,
            }));
        }
        let account = self.require_account(client)?;
        if account.locked {
            return Ok(ApplyOutcome::Rejected(RejectionReason::AccountLocked {
                client,
            }));
        }
        if deposit.status != DepositStatus::Disputed {
            return Ok(ApplyOutcome::Rejected(
                RejectionReason::DepositNotDisputed { tx },
            ));
        }

        let mut updated = account;
        updated.available += deposit.amount;
        updated.held -= deposit.amount;
        let changes =
            LedgerChanges::new()
                .with_account(updated)
                .with_deposit(DepositChange::UpdateStatus {
                    tx,
                    new_status: DepositStatus::Applied,
                });
        self.repository.commit(changes).map_err(repo)?;
        Ok(ApplyOutcome::Applied)
    }

    fn chargeback(
        &mut self,
        client: ClientId,
        tx: TransactionId,
    ) -> Result<ApplyOutcome, EngineError<R::Error>> {
        let Some(deposit) = self.repository.deposit(tx).map_err(repo)? else {
            return Ok(ApplyOutcome::Rejected(RejectionReason::DepositNotFound {
                tx,
            }));
        };
        if deposit.client_id != client {
            return Ok(ApplyOutcome::Rejected(RejectionReason::ClientMismatch {
                tx,
                expected_client: deposit.client_id,
                actual_client: client,
            }));
        }
        // Chargeback does not check `account.locked`: it can only fire on a
        // Disputed deposit, and Disputed->ChargedBack is the terminal transition.
        // Status guards prevent double chargeback.
        if deposit.status != DepositStatus::Disputed {
            return Ok(ApplyOutcome::Rejected(
                RejectionReason::DepositNotDisputed { tx },
            ));
        }
        let account = self.require_account(client)?;

        let mut updated = account;
        updated.held -= deposit.amount;
        updated.locked = true;
        let changes =
            LedgerChanges::new()
                .with_account(updated)
                .with_deposit(DepositChange::UpdateStatus {
                    tx,
                    new_status: DepositStatus::ChargedBack,
                });
        self.repository.commit(changes).map_err(repo)?;
        Ok(ApplyOutcome::Applied)
    }

    fn load_or_new_account(&self, client: ClientId) -> Result<Account, EngineError<R::Error>> {
        Ok(self
            .repository
            .account(client)
            .map_err(repo)?
            .unwrap_or_else(|| Account::new(client)))
    }

    fn require_account(&self, client: ClientId) -> Result<Account, EngineError<R::Error>> {
        self.repository
            .account(client)
            .map_err(repo)?
            .ok_or(EngineError::InvariantViolation(
                "deposit exists without owning account",
            ))
    }
}

fn repo<E>(e: E) -> EngineError<E>
where
    E: Debug + Display,
{
    EngineError::Repository(e)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::changes::AccountChange;
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
                    .insert(account.client_id, account.clone());
            }
            if let Some(tx) = changes.reserve_transaction_id {
                self.seen.borrow_mut().insert(tx);
            }
            match &changes.deposit {
                Some(DepositChange::Insert(deposit)) => {
                    self.deposits
                        .borrow_mut()
                        .insert(deposit.transaction_id, deposit.clone());
                }
                Some(DepositChange::UpdateStatus { tx, new_status }) => {
                    if let Some(record) = self.deposits.borrow_mut().get_mut(tx) {
                        record.status = *new_status;
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

    // Scenario 1
    #[test]
    fn applied_deposit_credits_available_reserves_tx_and_stores_record() {
        let mut e = engine();
        let out = e
            .process(Transaction::Deposit {
                client: 1,
                tx: 1,
                amount: dec!(10.0000),
            })
            .unwrap();
        assert_eq!(out, ApplyOutcome::Applied);
        let acc = e.repository().account(1).unwrap().unwrap();
        assert_eq!(acc.available, dec!(10.0000));
        assert_eq!(acc.held, Decimal::ZERO);
        assert_eq!(acc.total(), dec!(10.0000));
        assert!(!acc.locked);
        assert!(e.repository().transaction_seen(1).unwrap());
        assert!(e.repository().deposit(1).unwrap().is_some());
    }

    // Scenario 2
    #[test]
    fn applied_withdrawal_debits_available_reserves_tx_and_stores_no_record() {
        let mut e = engine();
        e.process(Transaction::Deposit {
            client: 1,
            tx: 1,
            amount: dec!(10.0000),
        })
        .unwrap();
        let out = e
            .process(Transaction::Withdrawal {
                client: 1,
                tx: 2,
                amount: dec!(3.0000),
            })
            .unwrap();
        assert_eq!(out, ApplyOutcome::Applied);
        let acc = e.repository().account(1).unwrap().unwrap();
        assert_eq!(acc.available, dec!(7.0000));
        assert!(e.repository().transaction_seen(2).unwrap());
        assert!(e.repository().deposit(2).unwrap().is_none());
    }

    // Scenario 3
    #[test]
    fn insufficient_withdrawal_reserves_tx_and_does_not_change_balance() {
        let mut e = engine();
        e.process(Transaction::Deposit {
            client: 1,
            tx: 1,
            amount: dec!(1.0000),
        })
        .unwrap();
        let out = e
            .process(Transaction::Withdrawal {
                client: 1,
                tx: 2,
                amount: dec!(5.0000),
            })
            .unwrap();
        assert_eq!(
            out,
            ApplyOutcome::Rejected(RejectionReason::InsufficientFunds { client: 1 })
        );
        let acc = e.repository().account(1).unwrap().unwrap();
        assert_eq!(acc.available, dec!(1.0000));
        assert!(e.repository().transaction_seen(2).unwrap());
    }

    // Scenario 4
    #[test]
    fn reusing_seen_tx_is_rejected_as_duplicate_transaction() {
        let mut e = engine();
        e.process(Transaction::Deposit {
            client: 1,
            tx: 1,
            amount: dec!(1.0000),
        })
        .unwrap();
        let out = e
            .process(Transaction::Deposit {
                client: 1,
                tx: 1,
                amount: dec!(9.0000),
            })
            .unwrap();
        assert_eq!(
            out,
            ApplyOutcome::Rejected(RejectionReason::DuplicateTransaction { tx: 1 })
        );
        let acc = e.repository().account(1).unwrap().unwrap();
        assert_eq!(acc.available, dec!(1.0000));

        let out2 = e
            .process(Transaction::Withdrawal {
                client: 1,
                tx: 1,
                amount: dec!(0.1),
            })
            .unwrap();
        assert_eq!(
            out2,
            ApplyOutcome::Rejected(RejectionReason::DuplicateTransaction { tx: 1 })
        );
    }

    // Scenario 5
    #[test]
    fn dispute_moves_amount_from_available_to_held_and_sets_status_disputed() {
        let mut e = engine();
        e.process(Transaction::Deposit {
            client: 1,
            tx: 1,
            amount: dec!(10.0000),
        })
        .unwrap();
        let out = e
            .process(Transaction::Dispute { client: 1, tx: 1 })
            .unwrap();
        assert_eq!(out, ApplyOutcome::Applied);
        let acc = e.repository().account(1).unwrap().unwrap();
        assert_eq!(acc.available, Decimal::ZERO);
        assert_eq!(acc.held, dec!(10.0000));
        assert_eq!(acc.total(), dec!(10.0000));
        assert_eq!(
            e.repository().deposit(1).unwrap().unwrap().status,
            DepositStatus::Disputed
        );
    }

    // Scenario 6
    #[test]
    fn resolve_moves_held_back_to_available_and_returns_status_applied() {
        let mut e = engine();
        e.process(Transaction::Deposit {
            client: 1,
            tx: 1,
            amount: dec!(10.0000),
        })
        .unwrap();
        e.process(Transaction::Dispute { client: 1, tx: 1 })
            .unwrap();
        let out = e
            .process(Transaction::Resolve { client: 1, tx: 1 })
            .unwrap();
        assert_eq!(out, ApplyOutcome::Applied);
        let acc = e.repository().account(1).unwrap().unwrap();
        assert_eq!(acc.available, dec!(10.0000));
        assert_eq!(acc.held, Decimal::ZERO);
        assert_eq!(
            e.repository().deposit(1).unwrap().unwrap().status,
            DepositStatus::Applied
        );
    }

    // Scenario 7
    #[test]
    fn chargeback_removes_held_locks_account_and_marks_charged_back() {
        let mut e = engine();
        e.process(Transaction::Deposit {
            client: 1,
            tx: 1,
            amount: dec!(10.0000),
        })
        .unwrap();
        e.process(Transaction::Dispute { client: 1, tx: 1 })
            .unwrap();
        let out = e
            .process(Transaction::Chargeback { client: 1, tx: 1 })
            .unwrap();
        assert_eq!(out, ApplyOutcome::Applied);
        let acc = e.repository().account(1).unwrap().unwrap();
        assert_eq!(acc.available, Decimal::ZERO);
        assert_eq!(acc.held, Decimal::ZERO);
        assert_eq!(acc.total(), Decimal::ZERO);
        assert!(acc.locked);
        assert_eq!(
            e.repository().deposit(1).unwrap().unwrap().status,
            DepositStatus::ChargedBack
        );
    }

    // Scenario 8: resolve or chargeback before dispute is rejected without state change
    #[test]
    fn resolve_before_dispute_is_rejected_without_state_change() {
        let mut e = engine();
        e.process(Transaction::Deposit {
            client: 1,
            tx: 1,
            amount: dec!(10.0000),
        })
        .unwrap();
        let commits_before = e.repository().commit_count();
        let out = e
            .process(Transaction::Resolve { client: 1, tx: 1 })
            .unwrap();
        assert_eq!(
            out,
            ApplyOutcome::Rejected(RejectionReason::DepositNotDisputed { tx: 1 })
        );
        assert_eq!(e.repository().commit_count(), commits_before);

        let out = e
            .process(Transaction::Chargeback { client: 1, tx: 1 })
            .unwrap();
        assert_eq!(
            out,
            ApplyOutcome::Rejected(RejectionReason::DepositNotDisputed { tx: 1 })
        );
        assert_eq!(e.repository().commit_count(), commits_before);
    }

    // Scenario 9
    #[test]
    fn unknown_deposit_reference_is_rejected() {
        let mut e = engine();
        for tx_ref in [
            Transaction::Dispute { client: 1, tx: 42 },
            Transaction::Resolve { client: 1, tx: 42 },
            Transaction::Chargeback { client: 1, tx: 42 },
        ] {
            let out = e.process(tx_ref).unwrap();
            assert_eq!(
                out,
                ApplyOutcome::Rejected(RejectionReason::DepositNotFound { tx: 42 })
            );
        }
    }

    // Scenario 10
    #[test]
    fn cross_client_reference_is_rejected() {
        let mut e = engine();
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
                expected_client: 1,
                actual_client: 2,
            })
        );
    }

    // Scenario 11: duplicate dispute rejected by lifecycle state
    #[test]
    fn duplicate_dispute_is_rejected_by_lifecycle_state() {
        let mut e = engine();
        e.process(Transaction::Deposit {
            client: 1,
            tx: 1,
            amount: dec!(10.0000),
        })
        .unwrap();
        e.process(Transaction::Dispute { client: 1, tx: 1 })
            .unwrap();
        let out = e
            .process(Transaction::Dispute { client: 1, tx: 1 })
            .unwrap();
        assert_eq!(
            out,
            ApplyOutcome::Rejected(RejectionReason::DepositAlreadyDisputed { tx: 1 })
        );
    }

    // Scenario 12
    #[test]
    fn dispute_may_make_available_negative_after_prior_spending() {
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
        let out = e
            .process(Transaction::Dispute { client: 1, tx: 1 })
            .unwrap();
        assert_eq!(out, ApplyOutcome::Applied);
        let acc = e.repository().account(1).unwrap().unwrap();
        assert_eq!(acc.available, dec!(-7.0000));
        assert_eq!(acc.held, dec!(10.0000));
        assert_eq!(acc.total(), dec!(3.0000));
    }

    // Scenario 13: new primary activity on a locked account is rejected and tx stays reserved
    #[test]
    fn primary_activity_on_locked_account_is_rejected_and_reserves_tx() {
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
        assert!(e.repository().account(1).unwrap().unwrap().locked);

        let out = e
            .process(Transaction::Deposit {
                client: 1,
                tx: 99,
                amount: dec!(5.0000),
            })
            .unwrap();
        assert_eq!(
            out,
            ApplyOutcome::Rejected(RejectionReason::AccountLocked { client: 1 })
        );
        assert!(e.repository().transaction_seen(99).unwrap());
        let acc = e.repository().account(1).unwrap().unwrap();
        assert_eq!(acc.total(), Decimal::ZERO);

        let out = e
            .process(Transaction::Withdrawal {
                client: 1,
                tx: 100,
                amount: dec!(1.0000),
            })
            .unwrap();
        assert_eq!(
            out,
            ApplyOutcome::Rejected(RejectionReason::AccountLocked { client: 1 })
        );
        assert!(e.repository().transaction_seen(100).unwrap());
    }

    // Additional: zero and negative amounts rejected as InvalidAmount, tx still reserved
    #[test]
    fn zero_or_negative_amount_is_rejected_as_invalid_but_reserves_tx() {
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

        let out = e
            .process(Transaction::Withdrawal {
                client: 1,
                tx: 2,
                amount: dec!(-1),
            })
            .unwrap();
        assert_eq!(out, ApplyOutcome::Rejected(RejectionReason::InvalidAmount));
        assert!(e.repository().transaction_seen(2).unwrap());
    }

    // Atomicity: one applied deposit produces one commit carrying all mutations
    #[test]
    fn applied_deposit_commits_all_mutations_in_one_change_set() {
        let mut e = engine();
        e.process(Transaction::Deposit {
            client: 1,
            tx: 1,
            amount: dec!(10.0000),
        })
        .unwrap();
        assert_eq!(e.repository().commit_count(), 1);
        let c = e.repository().last_commit();
        assert!(c.account.is_some());
        assert_eq!(c.reserve_transaction_id, Some(1));
        assert!(matches!(c.deposit, Some(DepositChange::Insert(_))));
    }
}
