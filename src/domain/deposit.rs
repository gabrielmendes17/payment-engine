use rust_decimal::Decimal;

use crate::domain::account::ClientId;
use crate::domain::errors::DepositError;
use crate::domain::transaction::TransactionId;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DepositStatus {
    Applied,
    Disputed,
    ChargedBack,
}

/// Only deposits are retained after processing; withdrawals have no
/// lifecycle events.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Deposit {
    transaction_id: TransactionId,
    client_id: ClientId,
    amount: Decimal,
    status: DepositStatus,
}

impl Deposit {
    pub fn new(
        transaction_id: TransactionId,
        client_id: ClientId,
        amount: Decimal,
    ) -> Result<Self, DepositError> {
        if amount <= Decimal::ZERO {
            return Err(DepositError::InvalidAmount);
        }
        Ok(Self {
            transaction_id,
            client_id,
            amount,
            status: DepositStatus::Applied,
        })
    }

    pub fn transaction_id(&self) -> TransactionId {
        self.transaction_id
    }

    pub fn client_id(&self) -> ClientId {
        self.client_id
    }

    pub fn amount(&self) -> Decimal {
        self.amount
    }

    pub fn status(&self) -> DepositStatus {
        self.status
    }

    pub fn ensure_owned_by(&self, client: ClientId) -> Result<(), DepositError> {
        if self.client_id == client {
            Ok(())
        } else {
            Err(DepositError::ClientMismatch {
                tx: self.transaction_id,
                owner_client: self.client_id,
                requesting_client: client,
            })
        }
    }

    pub(crate) fn begin_dispute(mut self) -> Result<Self, DepositError> {
        match self.status {
            DepositStatus::Applied => {
                self.status = DepositStatus::Disputed;
                Ok(self)
            }
            DepositStatus::Disputed => Err(DepositError::AlreadyDisputed {
                tx: self.transaction_id,
            }),
            DepositStatus::ChargedBack => Err(DepositError::AlreadyChargedBack {
                tx: self.transaction_id,
            }),
        }
    }

    pub(crate) fn resolve(mut self) -> Result<Self, DepositError> {
        match self.status {
            DepositStatus::Disputed => {
                self.status = DepositStatus::Applied;
                Ok(self)
            }
            DepositStatus::Applied => Err(DepositError::NotDisputed {
                tx: self.transaction_id,
            }),
            DepositStatus::ChargedBack => Err(DepositError::AlreadyChargedBack {
                tx: self.transaction_id,
            }),
        }
    }

    pub(crate) fn mark_charged_back(mut self) -> Result<Self, DepositError> {
        match self.status {
            DepositStatus::Disputed => {
                self.status = DepositStatus::ChargedBack;
                Ok(self)
            }
            DepositStatus::Applied => Err(DepositError::NotDisputed {
                tx: self.transaction_id,
            }),
            DepositStatus::ChargedBack => Err(DepositError::AlreadyChargedBack {
                tx: self.transaction_id,
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn applied(tx: TransactionId, client: ClientId) -> Deposit {
        Deposit::new(tx, client, dec!(10.0000)).unwrap()
    }

    #[test]
    fn new_starts_in_applied_status() {
        let d = applied(7, 1);
        assert_eq!(d.transaction_id(), 7);
        assert_eq!(d.client_id(), 1);
        assert_eq!(d.amount(), dec!(10.0000));
        assert_eq!(d.status(), DepositStatus::Applied);
    }

    #[test]
    fn new_rejects_zero_and_negative_amount() {
        use rust_decimal::Decimal;
        assert_eq!(
            Deposit::new(1, 1, Decimal::ZERO).unwrap_err(),
            DepositError::InvalidAmount
        );
        assert_eq!(
            Deposit::new(1, 1, dec!(-1)).unwrap_err(),
            DepositError::InvalidAmount
        );
    }

    #[test]
    fn ensure_owned_by_accepts_matching_client() {
        applied(1, 1).ensure_owned_by(1).unwrap();
    }

    #[test]
    fn ensure_owned_by_rejects_mismatched_client() {
        let err = applied(1, 1).ensure_owned_by(2).unwrap_err();
        assert_eq!(
            err,
            DepositError::ClientMismatch {
                tx: 1,
                owner_client: 1,
                requesting_client: 2,
            }
        );
    }

    #[test]
    fn begin_dispute_succeeds_from_applied() {
        let d = applied(1, 1).begin_dispute().unwrap();
        assert_eq!(d.status(), DepositStatus::Disputed);
    }

    #[test]
    fn begin_dispute_rejects_from_disputed_and_charged_back() {
        let disputed = applied(1, 1).begin_dispute().unwrap();
        assert_eq!(
            disputed.clone().begin_dispute().unwrap_err(),
            DepositError::AlreadyDisputed { tx: 1 }
        );
        let charged_back = disputed.mark_charged_back().unwrap();
        assert_eq!(
            charged_back.begin_dispute().unwrap_err(),
            DepositError::AlreadyChargedBack { tx: 1 }
        );
    }

    #[test]
    fn resolve_succeeds_from_disputed_only() {
        let disputed = applied(1, 1).begin_dispute().unwrap();
        let resolved = disputed.resolve().unwrap();
        assert_eq!(resolved.status(), DepositStatus::Applied);

        assert_eq!(
            resolved.clone().resolve().unwrap_err(),
            DepositError::NotDisputed { tx: 1 }
        );

        let charged_back = resolved
            .begin_dispute()
            .unwrap()
            .mark_charged_back()
            .unwrap();
        assert_eq!(
            charged_back.resolve().unwrap_err(),
            DepositError::AlreadyChargedBack { tx: 1 }
        );
    }

    #[test]
    fn mark_charged_back_succeeds_from_disputed_only() {
        let disputed = applied(1, 1).begin_dispute().unwrap();
        let charged_back = disputed.mark_charged_back().unwrap();
        assert_eq!(charged_back.status(), DepositStatus::ChargedBack);

        assert_eq!(
            charged_back.mark_charged_back().unwrap_err(),
            DepositError::AlreadyChargedBack { tx: 1 }
        );

        assert_eq!(
            applied(2, 1).mark_charged_back().unwrap_err(),
            DepositError::NotDisputed { tx: 2 }
        );
    }

    #[test]
    fn begin_dispute_succeeds_again_after_resolve() {
        let redisputed = applied(1, 1)
            .begin_dispute()
            .unwrap()
            .resolve()
            .unwrap()
            .begin_dispute()
            .unwrap();
        assert_eq!(redisputed.status(), DepositStatus::Disputed);
    }
}
