use rust_decimal::Decimal;

use crate::domain::account::ClientId;
use crate::domain::outcome::RejectionReason;
use crate::domain::transaction::TransactionId;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DepositStatus {
    Applied,
    Disputed,
    ChargedBack,
}

/// Persisted record of a deposit that participates in the dispute
/// lifecycle. Only deposits are retained after processing; withdrawals do
/// not have lifecycle events (see specs/01-domain-model.md).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DepositRecord {
    transaction_id: TransactionId,
    client_id: ClientId,
    amount: Decimal,
    status: DepositStatus,
}

impl DepositRecord {
    pub fn new_applied(
        transaction_id: TransactionId,
        client_id: ClientId,
        amount: Decimal,
    ) -> Self {
        Self {
            transaction_id,
            client_id,
            amount,
            status: DepositStatus::Applied,
        }
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

    /// Ownership guard used by dispute/resolve/chargeback flows.
    pub fn ensure_owned_by(&self, client: ClientId) -> Result<(), RejectionReason> {
        if self.client_id == client {
            Ok(())
        } else {
            Err(RejectionReason::ClientMismatch {
                tx: self.transaction_id,
                expected_client: self.client_id,
                actual_client: client,
            })
        }
    }

    /// Transition Applied -> Disputed.
    pub fn begin_dispute(mut self) -> Result<Self, RejectionReason> {
        match self.status {
            DepositStatus::Applied => {
                self.status = DepositStatus::Disputed;
                Ok(self)
            }
            DepositStatus::Disputed => Err(RejectionReason::DepositAlreadyDisputed {
                tx: self.transaction_id,
            }),
            DepositStatus::ChargedBack => Err(RejectionReason::DepositAlreadyChargedBack {
                tx: self.transaction_id,
            }),
        }
    }

    /// Transition Disputed -> Applied.
    pub fn resolve(mut self) -> Result<Self, RejectionReason> {
        if self.status != DepositStatus::Disputed {
            return Err(RejectionReason::DepositNotDisputed {
                tx: self.transaction_id,
            });
        }
        self.status = DepositStatus::Applied;
        Ok(self)
    }

    /// Terminal transition Disputed -> ChargedBack.
    pub fn charge_back(mut self) -> Result<Self, RejectionReason> {
        if self.status != DepositStatus::Disputed {
            return Err(RejectionReason::DepositNotDisputed {
                tx: self.transaction_id,
            });
        }
        self.status = DepositStatus::ChargedBack;
        Ok(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn applied(tx: TransactionId, client: ClientId) -> DepositRecord {
        DepositRecord::new_applied(tx, client, dec!(10.0000))
    }

    #[test]
    fn new_applied_starts_in_applied_status() {
        let d = applied(7, 1);
        assert_eq!(d.transaction_id(), 7);
        assert_eq!(d.client_id(), 1);
        assert_eq!(d.amount(), dec!(10.0000));
        assert_eq!(d.status(), DepositStatus::Applied);
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
            RejectionReason::ClientMismatch {
                tx: 1,
                expected_client: 1,
                actual_client: 2,
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
            RejectionReason::DepositAlreadyDisputed { tx: 1 }
        );
        let charged_back = disputed.charge_back().unwrap();
        assert_eq!(
            charged_back.begin_dispute().unwrap_err(),
            RejectionReason::DepositAlreadyChargedBack { tx: 1 }
        );
    }

    #[test]
    fn resolve_succeeds_from_disputed_only() {
        let disputed = applied(1, 1).begin_dispute().unwrap();
        let resolved = disputed.resolve().unwrap();
        assert_eq!(resolved.status(), DepositStatus::Applied);

        // Applied -> resolve is rejected
        assert_eq!(
            resolved.resolve().unwrap_err(),
            RejectionReason::DepositNotDisputed { tx: 1 }
        );
    }

    #[test]
    fn charge_back_succeeds_from_disputed_only() {
        let disputed = applied(1, 1).begin_dispute().unwrap();
        let charged_back = disputed.charge_back().unwrap();
        assert_eq!(charged_back.status(), DepositStatus::ChargedBack);

        // Disputed already consumed; a second charge_back on a non-disputed
        // record must fail.
        assert_eq!(
            charged_back.charge_back().unwrap_err(),
            RejectionReason::DepositNotDisputed { tx: 1 }
        );
    }
}
