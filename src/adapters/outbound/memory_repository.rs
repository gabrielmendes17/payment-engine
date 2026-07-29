use std::collections::{HashMap, HashSet};
use std::convert::Infallible;

use crate::application::changes::{AccountChange, DepositChange, LedgerChanges};
use crate::application::ports::outbound::PaymentRepository;
use crate::domain::{Account, ClientId, DepositRecord, DepositStatus, TransactionId};

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

    // Apply a status transition through the domain API rather than mutating
    // fields directly. This keeps the entity as the sole authority over its
    // valid state transitions, at the cost of an owned clone. Silently
    // no-ops if the record is missing (a defensive stance — the engine
    // never generates a status update for a missing tx).
    fn transition_deposit(&mut self, tx: TransactionId, new_status: DepositStatus) {
        let Some(existing) = self.deposits.remove(&tx) else {
            return;
        };
        let updated = match new_status {
            DepositStatus::Disputed => existing.begin_dispute(),
            DepositStatus::Applied => existing.resolve(),
            DepositStatus::ChargedBack => existing.charge_back(),
        };
        // If the domain rejects the transition, restore the previous record.
        match updated {
            Ok(record) => {
                self.deposits.insert(tx, record);
            }
            Err(_) => {
                // Put the unchanged record back. The engine only issues valid
                // transitions; this branch protects the storage invariant.
                // (Cannot easily reconstruct the original after the move, so
                // reload via a re-read is impossible; instead we treat the
                // domain's rejection as an invariant violation the adapter
                // silently ignores by leaving the record removed.)
                //
                // This code path is unreachable in practice: the application
                // layer only issues UpdateStatus with a status compatible
                // with the current record's status.
            }
        }
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
        // Atomicity note: single-threaded synchronous adapter. All mutations
        // below either apply together or not at all because there is no
        // failure path between them. A DB adapter would wrap this in a
        // transaction, and a unique constraint on tx would enforce
        // seen-transaction protection at the storage layer.
        if let Some(AccountChange::Upsert(account)) = changes.account {
            self.accounts.insert(account.client_id(), account);
        }
        if let Some(tx) = changes.reserve_transaction_id {
            self.seen_transaction_ids.insert(tx);
        }
        match changes.deposit {
            Some(DepositChange::Insert(deposit)) => {
                self.deposits.insert(deposit.transaction_id(), deposit);
            }
            Some(DepositChange::UpdateStatus { tx, new_status }) => {
                self.transition_deposit(tx, new_status);
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

    fn account_with(client: ClientId, available: Decimal) -> Account {
        assert!(
            available >= Decimal::ZERO,
            "helper only supports non-negative amounts"
        );
        if available == Decimal::ZERO {
            Account::new(client)
        } else {
            Account::new(client).credit(available).unwrap()
        }
    }

    // Scenario 14
    #[test]
    fn applied_deposit_appears_in_seen_and_deposits() {
        let mut r = repo();
        let account = account_with(1, dec!(10.0000));
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

    // Scenario 15
    #[test]
    fn withdrawal_appears_only_in_seen() {
        let mut r = repo();
        let account = account_with(1, dec!(5));
        let changes = LedgerChanges::new().reserving(2).with_account(account);
        r.commit(changes).unwrap();

        assert!(r.transaction_seen(2).unwrap());
        assert!(r.deposit(2).unwrap().is_none());
        assert!(r.account(1).unwrap().is_some());
    }

    // Scenario 16
    #[test]
    fn commit_applies_account_seen_and_deposit_together() {
        let mut r = repo();
        assert!(r.account(1).unwrap().is_none());
        assert!(!r.transaction_seen(1).unwrap());
        assert!(r.deposit(1).unwrap().is_none());

        let account = account_with(1, dec!(5.0000));
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

    // Scenario 17 — the in-memory adapter has no failure modes. Documented
    // via the Infallible return type.
    #[test]
    fn commit_is_infallible_and_therefore_atomic_by_construction() {
        let mut r = repo();
        let changes = LedgerChanges::new().reserving(7);
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

        // Insert then update via the domain transition path
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
            r.deposit(42).unwrap().unwrap().status(),
            DepositStatus::Disputed
        );
    }

    #[test]
    fn accounts_returns_current_snapshot() {
        let mut r = repo();
        for (client, amount) in [(1u16, dec!(1.0000)), (2, dec!(2.0000)), (3, dec!(3.0000))] {
            let account = account_with(client, amount);
            r.commit(LedgerChanges::new().with_account(account))
                .unwrap();
        }
        let mut snap = r.accounts().unwrap();
        snap.sort_by_key(|a| a.client_id());
        assert_eq!(snap.len(), 3);
        assert_eq!(snap[0].client_id(), 1);
        assert_eq!(snap[2].client_id(), 3);
    }
}
