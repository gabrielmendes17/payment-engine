use crate::domain::account::Account;
use crate::domain::deposit::Deposit;
use crate::domain::errors::DisputeError;

/// Rejection ordering: ownership → account locked → deposit status.
/// Account runs before deposit here because `Account::hold` has no status
/// precondition. The reverse ordering is required in `apply_resolve` and
/// `apply_chargeback` where `release`/`mark_charged_back` enforce
/// `held >= amount`, which would otherwise mask `NotDisputed`.
pub fn apply_dispute(
    account: Account,
    deposit: Deposit,
) -> Result<(Account, Deposit), DisputeError> {
    deposit.ensure_owned_by(account.client_id())?;
    let amount = deposit.amount();
    let updated_account = account.hold(amount)?;
    let updated_deposit = deposit.begin_dispute()?;
    Ok((updated_account, updated_deposit))
}

pub fn apply_resolve(
    account: Account,
    deposit: Deposit,
) -> Result<(Account, Deposit), DisputeError> {
    deposit.ensure_owned_by(account.client_id())?;
    let amount = deposit.amount();
    let updated_deposit = deposit.resolve()?;
    let updated_account = account.release(amount)?;
    Ok((updated_account, updated_deposit))
}

pub fn apply_chargeback(
    account: Account,
    deposit: Deposit,
) -> Result<(Account, Deposit), DisputeError> {
    deposit.ensure_owned_by(account.client_id())?;
    let amount = deposit.amount();
    let updated_deposit = deposit.mark_charged_back()?;
    let updated_account = account.mark_charged_back(amount)?;
    Ok((updated_account, updated_deposit))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::deposit::DepositStatus;
    use crate::domain::errors::{AccountError, DepositError};
    use rust_decimal::Decimal;
    use rust_decimal_macros::dec;

    fn applied_deposit() -> Deposit {
        Deposit::new(1, 1, dec!(10.0000)).unwrap()
    }

    fn account_with_available(available: Decimal) -> Account {
        Account::new(1).credit(available).unwrap()
    }

    fn locked_account_with_held(held: Decimal) -> Account {
        let account = Account::new(1).credit(held).unwrap();
        let deposit = Deposit::new(999, 1, held).unwrap();
        let (account, deposit) = apply_dispute(account, deposit).unwrap();
        let (account, _) = apply_chargeback(account, deposit).unwrap();
        account
    }

    #[test]
    fn apply_dispute_happy_path_moves_available_to_held_and_marks_disputed() {
        let account = account_with_available(dec!(10));
        let (account, deposit) = apply_dispute(account, applied_deposit()).unwrap();
        assert_eq!(account.available(), Decimal::ZERO);
        assert_eq!(account.held(), dec!(10));
        assert_eq!(deposit.status(), DepositStatus::Disputed);
    }

    #[test]
    fn apply_dispute_rejects_ownership_mismatch() {
        let account = Account::new(2).credit(dec!(10)).unwrap();
        let deposit = applied_deposit();
        let err = apply_dispute(account, deposit).unwrap_err();
        assert_eq!(
            err,
            DisputeError::Deposit(DepositError::ClientMismatch {
                tx: 1,
                owner_client: 1,
                requesting_client: 2,
            })
        );
    }

    #[test]
    fn apply_dispute_rejects_when_account_locked() {
        let account = locked_account_with_held(dec!(10));
        let deposit = applied_deposit();
        let err = apply_dispute(account, deposit).unwrap_err();
        assert_eq!(
            err,
            DisputeError::Account(AccountError::Locked { client: 1 })
        );
    }

    #[test]
    fn apply_dispute_rejects_double_dispute() {
        let account = account_with_available(dec!(10));
        let (account, deposit) = apply_dispute(account, applied_deposit()).unwrap();
        let err = apply_dispute(account, deposit).unwrap_err();
        assert_eq!(
            err,
            DisputeError::Deposit(DepositError::AlreadyDisputed { tx: 1 })
        );
    }

    #[test]
    fn apply_resolve_returns_held_to_available_and_marks_applied() {
        let account = account_with_available(dec!(10));
        let (account, deposit) = apply_dispute(account, applied_deposit()).unwrap();
        let (account, deposit) = apply_resolve(account, deposit).unwrap();
        assert_eq!(account.available(), dec!(10));
        assert_eq!(account.held(), Decimal::ZERO);
        assert_eq!(deposit.status(), DepositStatus::Applied);
    }

    #[test]
    fn apply_resolve_rejects_before_dispute() {
        let account = account_with_available(dec!(10));
        let err = apply_resolve(account, applied_deposit()).unwrap_err();
        assert_eq!(
            err,
            DisputeError::Deposit(DepositError::NotDisputed { tx: 1 })
        );
    }

    #[test]
    fn apply_chargeback_removes_held_locks_account_and_marks_charged_back() {
        let account = account_with_available(dec!(10));
        let (account, deposit) = apply_dispute(account, applied_deposit()).unwrap();
        let (account, deposit) = apply_chargeback(account, deposit).unwrap();
        assert_eq!(account.available(), Decimal::ZERO);
        assert_eq!(account.held(), Decimal::ZERO);
        assert_eq!(account.total(), Decimal::ZERO);
        assert!(account.is_locked());
        assert_eq!(deposit.status(), DepositStatus::ChargedBack);
    }

    #[test]
    fn apply_chargeback_rejects_before_dispute() {
        let account = account_with_available(dec!(10));
        let err = apply_chargeback(account, applied_deposit()).unwrap_err();
        assert_eq!(
            err,
            DisputeError::Deposit(DepositError::NotDisputed { tx: 1 })
        );
    }

    /// Locked account carrying residual held funds for a second disputed
    /// deposit. Built via pub(crate) domain calls because `apply_dispute`
    /// won't proceed on an already-locked account.
    fn locked_account_with_residual_held() -> Account {
        Account::new(1)
            .credit(dec!(15))
            .unwrap()
            .hold(dec!(10))
            .unwrap()
            .hold(dec!(5))
            .unwrap()
            .mark_charged_back(dec!(10))
            .unwrap()
    }

    fn disputed_deposit(tx: u32, amount: Decimal) -> Deposit {
        Deposit::new(tx, 1, amount)
            .unwrap()
            .begin_dispute()
            .unwrap()
    }

    #[test]
    fn apply_chargeback_rejects_on_locked_account_for_a_different_deposit() {
        let account = locked_account_with_residual_held();
        assert!(account.is_locked() && account.held() == dec!(5));
        let second = disputed_deposit(42, dec!(5));
        let err = apply_chargeback(account, second).unwrap_err();
        assert_eq!(
            err,
            DisputeError::Account(AccountError::Locked { client: 1 })
        );
    }
}
