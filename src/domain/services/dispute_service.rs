//! Pure domain coordination for the dispute lifecycle.
//!
//! Each function takes owned `Account` and `DepositRecord` values, applies
//! the relevant rule combining both, and returns the two updated entities.
//! No repository access, no `LedgerChanges`, no I/O. This layer exists so
//! the application use cases can stay focused on orchestration.

use crate::domain::account::Account;
use crate::domain::deposit::DepositRecord;
use crate::domain::outcome::RejectionReason;

/// Apply a dispute on `deposit` for `account`.
///
/// Rejection ordering preserved from specs/02-processing-rules.md:
/// ownership → account locked → deposit status.
pub fn apply_dispute(
    account: Account,
    deposit: DepositRecord,
) -> Result<(Account, DepositRecord), RejectionReason> {
    deposit.ensure_owned_by(account.client_id())?;
    let amount = deposit.amount();
    let updated_account = account.hold(amount)?;
    let updated_deposit = deposit.begin_dispute()?;
    Ok((updated_account, updated_deposit))
}

/// Apply a resolve on `deposit` for `account`.
///
/// Rejection ordering: ownership → account locked → deposit status.
pub fn apply_resolve(
    account: Account,
    deposit: DepositRecord,
) -> Result<(Account, DepositRecord), RejectionReason> {
    deposit.ensure_owned_by(account.client_id())?;
    let amount = deposit.amount();
    let updated_account = account.release(amount)?;
    let updated_deposit = deposit.resolve()?;
    Ok((updated_account, updated_deposit))
}

/// Apply a chargeback on `deposit` for `account`.
///
/// Ownership is checked first, then the deposit transitions
/// Disputed -> ChargedBack, then the account is chargeback-updated.
/// The account lock is NOT checked: chargeback can only fire from
/// `Disputed`, which prevents double execution.
pub fn apply_chargeback(
    account: Account,
    deposit: DepositRecord,
) -> Result<(Account, DepositRecord), RejectionReason> {
    deposit.ensure_owned_by(account.client_id())?;
    let amount = deposit.amount();
    let updated_deposit = deposit.charge_back()?;
    let updated_account = account.apply_chargeback(amount);
    Ok((updated_account, updated_deposit))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::deposit::DepositStatus;
    use rust_decimal::Decimal;
    use rust_decimal_macros::dec;

    fn applied_deposit() -> DepositRecord {
        DepositRecord::new_applied(1, 1, dec!(10.0000))
    }

    fn account_with_available(available: Decimal) -> Account {
        Account::new(1).credit(available).unwrap()
    }

    fn locked_account_with_held(held: Decimal) -> Account {
        // Fund the account, dispute a synthetic deposit to move funds into
        // held, then chargeback to lock. This drives the entity only through
        // its public API.
        let account = Account::new(1).credit(held).unwrap();
        let deposit = DepositRecord::new_applied(999, 1, held);
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
        let deposit = applied_deposit(); // client 1
        let err = apply_dispute(account, deposit).unwrap_err();
        assert_eq!(
            err,
            RejectionReason::ClientMismatch {
                tx: 1,
                expected_client: 1,
                actual_client: 2,
            }
        );
    }

    #[test]
    fn apply_dispute_rejects_when_account_locked() {
        let account = locked_account_with_held(dec!(10));
        // account is now locked with held=0 available=0 total=0
        let deposit = applied_deposit();
        let err = apply_dispute(account, deposit).unwrap_err();
        assert_eq!(err, RejectionReason::AccountLocked { client: 1 });
    }

    #[test]
    fn apply_dispute_rejects_double_dispute() {
        let account = account_with_available(dec!(10));
        let (account, deposit) = apply_dispute(account, applied_deposit()).unwrap();
        let err = apply_dispute(account, deposit).unwrap_err();
        assert_eq!(err, RejectionReason::DepositAlreadyDisputed { tx: 1 });
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
        assert_eq!(err, RejectionReason::DepositNotDisputed { tx: 1 });
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
        assert_eq!(err, RejectionReason::DepositNotDisputed { tx: 1 });
    }
}
