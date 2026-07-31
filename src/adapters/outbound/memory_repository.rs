use std::collections::{HashMap, HashSet};
use std::convert::Infallible;

use crate::application::changes::LedgerChanges;
use crate::application::ports::outbound::LedgerRepository;
use crate::domain::{Account, ClientId, Deposit, TransactionId};

#[derive(Debug, Default)]
pub struct InMemoryLedgerRepository {
    // HashMap because the challenge states output row order does not matter,
    // so we optimize for O(1) lookup on the hot path. If deterministic output
    // ever became required, sorting at the serialization boundary would cost
    // O(n log n) once per run without changing this storage shape.
    accounts: HashMap<ClientId, Account>,
    seen_transaction_ids: HashSet<TransactionId>,
    deposits: HashMap<TransactionId, Deposit>,
}

impl InMemoryLedgerRepository {
    pub fn new() -> Self {
        Self::default()
    }
}

impl LedgerRepository for InMemoryLedgerRepository {
    type Error = Infallible;

    fn transaction_seen(&self, tx: TransactionId) -> Result<bool, Self::Error> {
        Ok(self.seen_transaction_ids.contains(&tx))
    }

    fn account(&self, client: ClientId) -> Result<Option<Account>, Self::Error> {
        Ok(self.accounts.get(&client).cloned())
    }

    fn deposit(&self, tx: TransactionId) -> Result<Option<Deposit>, Self::Error> {
        Ok(self.deposits.get(&tx).cloned())
    }

    fn commit(&mut self, changes: LedgerChanges) -> Result<(), Self::Error> {
        // Pure upsert: the domain produced fully-updated entities and this
        // single-threaded adapter has no failure path between mutations.
        let account = changes.account;
        self.accounts.insert(account.client_id(), account);
        if let Some(tx) = changes.reserve_transaction_id {
            self.seen_transaction_ids.insert(tx);
        }
        if let Some(deposit) = changes.deposit {
            self.deposits.insert(deposit.transaction_id(), deposit);
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
    use crate::domain::{Account, Deposit, DepositStatus};
    use rust_decimal::Decimal;
    use rust_decimal_macros::dec;

    fn repo() -> InMemoryLedgerRepository {
        InMemoryLedgerRepository::new()
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

    #[test]
    fn applied_deposit_appears_in_seen_and_deposits() {
        let mut r = repo();
        let account = account_with(1, dec!(10.0000));
        let deposit = Deposit::new(1, 1, dec!(10.0000)).unwrap();
        let changes = LedgerChanges::new(account)
            .reserving(1)
            .with_deposit(deposit);
        r.commit(changes).unwrap();

        assert!(r.transaction_seen(1).unwrap());
        assert!(r.deposit(1).unwrap().is_some());
        assert!(r.account(1).unwrap().is_some());
    }

    #[test]
    fn withdrawal_appears_only_in_seen() {
        let mut r = repo();
        let account = account_with(1, dec!(5));
        let changes = LedgerChanges::new(account).reserving(2);
        r.commit(changes).unwrap();

        assert!(r.transaction_seen(2).unwrap());
        assert!(r.deposit(2).unwrap().is_none());
        assert!(r.account(1).unwrap().is_some());
    }

    #[test]
    fn commit_applies_account_seen_and_deposit_together() {
        let mut r = repo();
        assert!(r.account(1).unwrap().is_none());
        assert!(!r.transaction_seen(1).unwrap());
        assert!(r.deposit(1).unwrap().is_none());

        let account = account_with(1, dec!(5.0000));
        let deposit = Deposit::new(1, 1, dec!(5.0000)).unwrap();
        let changes = LedgerChanges::new(account.clone())
            .reserving(1)
            .with_deposit(deposit.clone());
        r.commit(changes).unwrap();

        assert_eq!(r.account(1).unwrap(), Some(account));
        assert!(r.transaction_seen(1).unwrap());
        assert_eq!(r.deposit(1).unwrap(), Some(deposit));
    }

    #[test]
    fn commit_is_infallible_and_therefore_atomic_by_construction() {
        let mut r = repo();
        let changes = LedgerChanges::new(account_with(1, dec!(0))).reserving(7);
        let res: Result<(), Infallible> = r.commit(changes);
        res.unwrap();
        assert!(r.transaction_seen(7).unwrap());
    }

    #[test]
    fn upsert_replaces_existing_deposit_with_new_status() {
        let mut r = repo();
        let account = account_with(1, dec!(1.0000));
        let deposit = Deposit::new(42, 1, dec!(1.0000)).unwrap();
        r.commit(
            LedgerChanges::new(account.clone())
                .reserving(42)
                .with_deposit(deposit.clone()),
        )
        .unwrap();
        assert_eq!(
            r.deposit(42).unwrap().unwrap().status(),
            DepositStatus::Applied
        );

        let disputed = deposit.begin_dispute().unwrap();
        r.commit(LedgerChanges::new(account).with_deposit(disputed))
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
            r.commit(LedgerChanges::new(account)).unwrap();
        }
        let mut snap = r.accounts().unwrap();
        snap.sort_by_key(|a| a.client_id());
        assert_eq!(snap.len(), 3);
        assert_eq!(snap[0].client_id(), 1);
        assert_eq!(snap[2].client_id(), 3);
    }
}
