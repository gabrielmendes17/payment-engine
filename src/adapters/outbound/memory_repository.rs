use std::collections::{HashMap, HashSet};
use std::convert::Infallible;

use crate::application::changes::{AccountChange, DepositChange, LedgerChanges};
use crate::application::ports::outbound::PaymentRepository;
use crate::domain::{Account, ClientId, DepositRecord, TransactionId};

#[derive(Debug, Default)]
pub struct InMemoryPaymentRepository {
    accounts: HashMap<ClientId, Account>,
    seen_transaction_ids: HashSet<TransactionId>,
    deposits: HashMap<TransactionId, DepositRecord>,
}

impl InMemoryPaymentRepository {
    pub fn new() -> Self {
        Self::default()
    }
}

impl PaymentRepository for InMemoryPaymentRepository {
    // The in-memory adapter has no failure modes today. `Infallible` documents
    // that. A future database adapter would introduce its own error type; the
    // application layer already carries `R::Error` generically so no code in
    // `PaymentEngine` needs to change.
    type Error = Infallible;

    fn transaction_seen(&self, tx: TransactionId) -> Result<bool, Self::Error> {
        Ok(self.seen_transaction_ids.contains(&tx))
    }

    fn account(&self, client: ClientId) -> Result<Option<Account>, Self::Error> {
        Ok(self.accounts.get(&client).cloned())
    }

    fn deposit(&self, tx: TransactionId) -> Result<Option<DepositRecord>, Self::Error> {
        Ok(self.deposits.get(&tx).cloned())
    }

    fn commit(&mut self, changes: LedgerChanges) -> Result<(), Self::Error> {
        // Atomicity note: single-threaded synchronous adapter. All three
        // mutations below either apply together or not at all because there
        // is no failure path between them. A DB adapter would wrap this in a
        // transaction, and a unique constraint on tx would enforce
        // seen-transaction protection at the storage layer.
        if let Some(AccountChange::Upsert(account)) = changes.account {
            self.accounts.insert(account.client_id, account);
        }
        if let Some(tx) = changes.reserve_transaction_id {
            self.seen_transaction_ids.insert(tx);
        }
        match changes.deposit {
            Some(DepositChange::Insert(deposit)) => {
                self.deposits.insert(deposit.transaction_id, deposit);
            }
            Some(DepositChange::UpdateStatus { tx, new_status }) => {
                if let Some(record) = self.deposits.get_mut(&tx) {
                    record.status = new_status;
                }
            }
            None => {}
        }
        Ok(())
    }

    fn accounts(&self) -> Result<Vec<Account>, Self::Error> {
        Ok(self.accounts.values().cloned().collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{Account, DepositRecord, DepositStatus};
    use rust_decimal::Decimal;
    use rust_decimal_macros::dec;

    fn repo() -> InMemoryPaymentRepository {
        InMemoryPaymentRepository::new()
    }

    // Scenario 14: an applied deposit's tx exists in both seen_transaction_ids and deposits
    #[test]
    fn applied_deposit_appears_in_seen_and_deposits() {
        let mut r = repo();
        let account = Account {
            client_id: 1,
            available: dec!(10.0000),
            held: Decimal::ZERO,
            locked: false,
        };
        let deposit = DepositRecord::new_applied(1, 1, dec!(10.0000));
        let changes = LedgerChanges::new()
            .reserving(1)
            .with_account(account)
            .with_deposit(DepositChange::Insert(deposit));
        r.commit(changes).unwrap();

        assert!(r.transaction_seen(1).unwrap());
        assert!(r.deposit(1).unwrap().is_some());
        assert!(r.account(1).unwrap().is_some());
    }

    // Scenario 15: a withdrawal tx exists only in seen_transaction_ids
    #[test]
    fn withdrawal_appears_only_in_seen() {
        let mut r = repo();
        let account = Account {
            client_id: 1,
            available: dec!(-5.0000),
            held: Decimal::ZERO,
            locked: false,
        };
        let changes = LedgerChanges::new().reserving(2).with_account(account);
        r.commit(changes).unwrap();

        assert!(r.transaction_seen(2).unwrap());
        assert!(r.deposit(2).unwrap().is_none());
        assert!(r.account(1).unwrap().is_some());
    }

    // Scenario 16: repository commit applies all event changes atomically
    #[test]
    fn commit_applies_account_seen_and_deposit_together() {
        let mut r = repo();
        assert!(r.account(1).unwrap().is_none());
        assert!(!r.transaction_seen(1).unwrap());
        assert!(r.deposit(1).unwrap().is_none());

        let account = Account {
            client_id: 1,
            available: dec!(5.0000),
            held: Decimal::ZERO,
            locked: false,
        };
        let deposit = DepositRecord::new_applied(1, 1, dec!(5.0000));
        let changes = LedgerChanges::new()
            .reserving(1)
            .with_account(account.clone())
            .with_deposit(DepositChange::Insert(deposit.clone()));
        r.commit(changes).unwrap();

        assert_eq!(r.account(1).unwrap(), Some(account));
        assert!(r.transaction_seen(1).unwrap());
        assert_eq!(r.deposit(1).unwrap(), Some(deposit));
    }

    // Scenario 17: a failed commit exposes no partial mutation.
    // The in-memory adapter has no failure modes: commit is Infallible and
    // Rust's type system prevents partial writes from being observable
    // across a commit boundary. The invariant is satisfied by construction.
    // A DB adapter must satisfy this via transactional writes.
    #[test]
    fn commit_is_infallible_and_therefore_atomic_by_construction() {
        let mut r = repo();
        let changes = LedgerChanges::new().reserving(7);
        // Type check: the return type is Result<(), Infallible>.
        let res: Result<(), Infallible> = r.commit(changes);
        res.unwrap();
        assert!(r.transaction_seen(7).unwrap());
    }

    #[test]
    fn update_status_only_mutates_when_deposit_exists() {
        let mut r = repo();
        // No deposit yet -> update should be a no-op
        let changes = LedgerChanges::new().with_deposit(DepositChange::UpdateStatus {
            tx: 42,
            new_status: DepositStatus::Disputed,
        });
        r.commit(changes).unwrap();
        assert!(r.deposit(42).unwrap().is_none());

        // Insert then update
        let deposit = DepositRecord::new_applied(42, 1, dec!(1.0000));
        r.commit(
            LedgerChanges::new()
                .reserving(42)
                .with_deposit(DepositChange::Insert(deposit)),
        )
        .unwrap();
        r.commit(
            LedgerChanges::new().with_deposit(DepositChange::UpdateStatus {
                tx: 42,
                new_status: DepositStatus::Disputed,
            }),
        )
        .unwrap();
        assert_eq!(
            r.deposit(42).unwrap().unwrap().status,
            DepositStatus::Disputed
        );
    }

    #[test]
    fn accounts_returns_current_snapshot() {
        let mut r = repo();
        for (client, amount) in [(1u16, dec!(1.0000)), (2, dec!(2.0000)), (3, dec!(3.0000))] {
            let account = Account {
                client_id: client,
                available: amount,
                held: Decimal::ZERO,
                locked: false,
            };
            r.commit(LedgerChanges::new().with_account(account))
                .unwrap();
        }
        let mut snap = r.accounts().unwrap();
        snap.sort_by_key(|a| a.client_id);
        assert_eq!(snap.len(), 3);
        assert_eq!(snap[0].client_id, 1);
        assert_eq!(snap[2].client_id, 3);
    }
}
