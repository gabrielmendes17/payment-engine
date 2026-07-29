use rust_decimal::Decimal;

use crate::domain::outcome::RejectionReason;

pub type ClientId = u16;

/// Aggregate root for a single client's balance and lock state.
///
/// Balance-changing methods take ownership of `self` and return an updated
/// `Account` on success. This makes each transition explicit at call sites
/// and mirrors the atomic-write model used by the outbound repository:
/// either the whole new state is committed, or none of it is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Account {
    client_id: ClientId,
    available: Decimal,
    held: Decimal,
    locked: bool,
}

impl Account {
    pub fn new(client_id: ClientId) -> Self {
        Self {
            client_id,
            available: Decimal::ZERO,
            held: Decimal::ZERO,
            locked: false,
        }
    }

    pub fn client_id(&self) -> ClientId {
        self.client_id
    }

    pub fn available(&self) -> Decimal {
        self.available
    }

    pub fn held(&self) -> Decimal {
        self.held
    }

    pub fn total(&self) -> Decimal {
        self.available + self.held
    }

    pub fn is_locked(&self) -> bool {
        self.locked
    }

    fn ensure_unlocked(&self) -> Result<(), RejectionReason> {
        if self.locked {
            Err(RejectionReason::AccountLocked {
                client: self.client_id,
            })
        } else {
            Ok(())
        }
    }

    fn ensure_positive(amount: Decimal) -> Result<(), RejectionReason> {
        if amount <= Decimal::ZERO {
            Err(RejectionReason::InvalidAmount)
        } else {
            Ok(())
        }
    }

    /// Credit funds to available (deposit path).
    pub fn credit(mut self, amount: Decimal) -> Result<Self, RejectionReason> {
        Self::ensure_positive(amount)?;
        self.ensure_unlocked()?;
        self.available += amount;
        Ok(self)
    }

    /// Debit funds from available (withdrawal path).
    pub fn debit(mut self, amount: Decimal) -> Result<Self, RejectionReason> {
        Self::ensure_positive(amount)?;
        self.ensure_unlocked()?;
        if self.available < amount {
            return Err(RejectionReason::InsufficientFunds {
                client: self.client_id,
            });
        }
        self.available -= amount;
        Ok(self)
    }

    /// Move value from available to held (dispute effect).
    /// May take `available` negative if the client has already spent the
    /// disputed funds (see specs/05-acceptance-scenarios.md scenario 12).
    pub fn hold(mut self, amount: Decimal) -> Result<Self, RejectionReason> {
        self.ensure_unlocked()?;
        self.available -= amount;
        self.held += amount;
        Ok(self)
    }

    /// Move value from held back to available (resolve effect).
    pub fn release(mut self, amount: Decimal) -> Result<Self, RejectionReason> {
        self.ensure_unlocked()?;
        self.available += amount;
        self.held -= amount;
        Ok(self)
    }

    /// Terminal chargeback effect: remove held funds and lock the account.
    /// This does not check the lock: the dispute-lifecycle status guard on
    /// the owning deposit prevents double chargeback.
    pub fn apply_chargeback(mut self, amount: Decimal) -> Self {
        self.held -= amount;
        self.locked = true;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn account(client_id: ClientId, available: Decimal, held: Decimal, locked: bool) -> Account {
        Account {
            client_id,
            available,
            held,
            locked,
        }
    }

    #[test]
    fn new_account_starts_at_zero_and_unlocked() {
        let a = Account::new(42);
        assert_eq!(a.client_id(), 42);
        assert_eq!(a.available(), Decimal::ZERO);
        assert_eq!(a.held(), Decimal::ZERO);
        assert_eq!(a.total(), Decimal::ZERO);
        assert!(!a.is_locked());
    }

    #[test]
    fn total_is_available_plus_held() {
        let a = account(1, dec!(10.1234), dec!(2.5000), false);
        assert_eq!(a.total(), dec!(12.6234));
    }

    #[test]
    fn credit_increases_available_when_positive_and_unlocked() {
        let a = Account::new(1).credit(dec!(10.0000)).unwrap();
        assert_eq!(a.available(), dec!(10.0000));
        assert_eq!(a.held(), Decimal::ZERO);
    }

    #[test]
    fn credit_rejects_zero_and_negative_amount() {
        let a = Account::new(1);
        assert_eq!(
            a.clone().credit(Decimal::ZERO).unwrap_err(),
            RejectionReason::InvalidAmount
        );
        assert_eq!(
            a.credit(dec!(-1)).unwrap_err(),
            RejectionReason::InvalidAmount
        );
    }

    #[test]
    fn credit_rejects_when_locked() {
        let a = account(1, Decimal::ZERO, Decimal::ZERO, true);
        assert_eq!(
            a.credit(dec!(1)).unwrap_err(),
            RejectionReason::AccountLocked { client: 1 }
        );
    }

    #[test]
    fn debit_decreases_available_when_sufficient_and_unlocked() {
        let a = account(1, dec!(10), Decimal::ZERO, false)
            .debit(dec!(3))
            .unwrap();
        assert_eq!(a.available(), dec!(7));
    }

    #[test]
    fn debit_rejects_zero_negative_locked_and_insufficient() {
        let unlocked = account(1, dec!(1), Decimal::ZERO, false);
        assert_eq!(
            unlocked.clone().debit(Decimal::ZERO).unwrap_err(),
            RejectionReason::InvalidAmount
        );
        assert_eq!(
            unlocked.clone().debit(dec!(-1)).unwrap_err(),
            RejectionReason::InvalidAmount
        );
        assert_eq!(
            unlocked.debit(dec!(5)).unwrap_err(),
            RejectionReason::InsufficientFunds { client: 1 }
        );
        let locked = account(1, dec!(10), Decimal::ZERO, true);
        assert_eq!(
            locked.debit(dec!(1)).unwrap_err(),
            RejectionReason::AccountLocked { client: 1 }
        );
    }

    #[test]
    fn hold_moves_value_from_available_to_held() {
        let a = account(1, dec!(10), Decimal::ZERO, false)
            .hold(dec!(4))
            .unwrap();
        assert_eq!(a.available(), dec!(6));
        assert_eq!(a.held(), dec!(4));
        assert_eq!(a.total(), dec!(10));
    }

    #[test]
    fn hold_may_make_available_negative_scenario_12() {
        let a = account(1, dec!(3), Decimal::ZERO, false)
            .hold(dec!(10))
            .unwrap();
        assert_eq!(a.available(), dec!(-7));
        assert_eq!(a.held(), dec!(10));
        assert_eq!(a.total(), dec!(3));
    }

    #[test]
    fn hold_rejects_when_locked() {
        let a = account(1, dec!(10), Decimal::ZERO, true);
        assert_eq!(
            a.hold(dec!(1)).unwrap_err(),
            RejectionReason::AccountLocked { client: 1 }
        );
    }

    #[test]
    fn release_moves_value_from_held_back_to_available() {
        let a = account(1, dec!(6), dec!(4), false)
            .release(dec!(4))
            .unwrap();
        assert_eq!(a.available(), dec!(10));
        assert_eq!(a.held(), Decimal::ZERO);
    }

    #[test]
    fn release_rejects_when_locked() {
        let a = account(1, dec!(6), dec!(4), true);
        assert_eq!(
            a.release(dec!(4)).unwrap_err(),
            RejectionReason::AccountLocked { client: 1 }
        );
    }

    #[test]
    fn apply_chargeback_removes_held_and_locks_regardless_of_prior_state() {
        let unlocked = account(1, Decimal::ZERO, dec!(10), false).apply_chargeback(dec!(10));
        assert_eq!(unlocked.available(), Decimal::ZERO);
        assert_eq!(unlocked.held(), Decimal::ZERO);
        assert!(unlocked.is_locked());
    }
}
