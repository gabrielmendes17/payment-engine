use rust_decimal::Decimal;

use crate::domain::errors::AccountError;

pub type ClientId = u16;

/// Balance-changing methods consume `self` and return an updated `Account`.
/// Low-level lifecycle transitions (`hold`, `release`, `mark_charged_back`)
/// are `pub(crate)` so external callers must go through `dispute_service`,
/// which enforces the ownership → lock → status ordering.
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

    fn ensure_unlocked(&self) -> Result<(), AccountError> {
        if self.locked {
            Err(AccountError::Locked {
                client: self.client_id,
            })
        } else {
            Ok(())
        }
    }

    fn ensure_positive(amount: Decimal) -> Result<(), AccountError> {
        if amount <= Decimal::ZERO {
            Err(AccountError::InvalidAmount)
        } else {
            Ok(())
        }
    }

    pub fn credit(mut self, amount: Decimal) -> Result<Self, AccountError> {
        Self::ensure_positive(amount)?;
        self.ensure_unlocked()?;
        self.available += amount;
        Ok(self)
    }

    pub fn debit(mut self, amount: Decimal) -> Result<Self, AccountError> {
        Self::ensure_positive(amount)?;
        self.ensure_unlocked()?;
        if self.available < amount {
            return Err(AccountError::InsufficientFunds {
                client: self.client_id,
            });
        }
        self.available -= amount;
        Ok(self)
    }

    /// May take `available` negative if the client has already spent the
    /// disputed funds; `total = available + held` is preserved.
    pub(crate) fn hold(mut self, amount: Decimal) -> Result<Self, AccountError> {
        Self::ensure_positive(amount)?;
        self.ensure_unlocked()?;
        self.available -= amount;
        self.held += amount;
        Ok(self)
    }

    pub(crate) fn release(mut self, amount: Decimal) -> Result<Self, AccountError> {
        Self::ensure_positive(amount)?;
        self.ensure_unlocked()?;
        if self.held < amount {
            return Err(AccountError::InsufficientHeldFunds {
                client: self.client_id,
            });
        }
        self.available += amount;
        self.held -= amount;
        Ok(self)
    }

    pub(crate) fn mark_charged_back(mut self, amount: Decimal) -> Result<Self, AccountError> {
        Self::ensure_positive(amount)?;
        self.ensure_unlocked()?;
        if self.held < amount {
            return Err(AccountError::InsufficientHeldFunds {
                client: self.client_id,
            });
        }
        self.held -= amount;
        self.locked = true;
        Ok(self)
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
            AccountError::InvalidAmount
        );
        assert_eq!(a.credit(dec!(-1)).unwrap_err(), AccountError::InvalidAmount);
    }

    #[test]
    fn credit_rejects_when_locked() {
        let a = account(1, Decimal::ZERO, Decimal::ZERO, true);
        assert_eq!(
            a.credit(dec!(1)).unwrap_err(),
            AccountError::Locked { client: 1 }
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
            AccountError::InvalidAmount
        );
        assert_eq!(
            unlocked.clone().debit(dec!(-1)).unwrap_err(),
            AccountError::InvalidAmount
        );
        assert_eq!(
            unlocked.debit(dec!(5)).unwrap_err(),
            AccountError::InsufficientFunds { client: 1 }
        );
        let locked = account(1, dec!(10), Decimal::ZERO, true);
        assert_eq!(
            locked.debit(dec!(1)).unwrap_err(),
            AccountError::Locked { client: 1 }
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
    fn hold_rejects_when_locked_or_invalid_amount() {
        let locked = account(1, dec!(10), Decimal::ZERO, true);
        assert_eq!(
            locked.hold(dec!(1)).unwrap_err(),
            AccountError::Locked { client: 1 }
        );
        let unlocked = account(1, dec!(10), Decimal::ZERO, false);
        assert_eq!(
            unlocked.clone().hold(Decimal::ZERO).unwrap_err(),
            AccountError::InvalidAmount
        );
        assert_eq!(
            unlocked.hold(dec!(-1)).unwrap_err(),
            AccountError::InvalidAmount
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
    fn release_rejects_when_locked_invalid_or_over_held() {
        let locked = account(1, dec!(6), dec!(4), true);
        assert_eq!(
            locked.release(dec!(4)).unwrap_err(),
            AccountError::Locked { client: 1 }
        );
        let unlocked = account(1, dec!(6), dec!(4), false);
        assert_eq!(
            unlocked.clone().release(Decimal::ZERO).unwrap_err(),
            AccountError::InvalidAmount
        );
        assert_eq!(
            unlocked.release(dec!(5)).unwrap_err(),
            AccountError::InsufficientHeldFunds { client: 1 }
        );
    }

    #[test]
    fn mark_charged_back_removes_held_and_locks_regardless_of_prior_state() {
        let unlocked = account(1, Decimal::ZERO, dec!(10), false)
            .mark_charged_back(dec!(10))
            .unwrap();
        assert_eq!(unlocked.available(), Decimal::ZERO);
        assert_eq!(unlocked.held(), Decimal::ZERO);
        assert!(unlocked.is_locked());
    }

    #[test]
    fn mark_charged_back_rejects_on_locked_account() {
        let locked = account(1, Decimal::ZERO, dec!(5), true);
        assert_eq!(
            locked.mark_charged_back(dec!(5)).unwrap_err(),
            AccountError::Locked { client: 1 }
        );
    }

    #[test]
    fn mark_charged_back_rejects_over_held_or_invalid() {
        let a = account(1, Decimal::ZERO, dec!(3), false);
        assert_eq!(
            a.clone().mark_charged_back(dec!(5)).unwrap_err(),
            AccountError::InsufficientHeldFunds { client: 1 }
        );
        assert_eq!(
            a.mark_charged_back(dec!(-1)).unwrap_err(),
            AccountError::InvalidAmount
        );
    }
}
