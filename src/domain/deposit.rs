use rust_decimal::Decimal;

use crate::domain::account::ClientId;
use crate::domain::transaction::TransactionId;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DepositStatus {
    Applied,
    Disputed,
    ChargedBack,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DepositRecord {
    pub transaction_id: TransactionId,
    pub client_id: ClientId,
    pub amount: Decimal,
    pub status: DepositStatus,
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn new_applied_starts_in_applied_status() {
        let d = DepositRecord::new_applied(7, 1, dec!(10.0000));
        assert_eq!(d.transaction_id, 7);
        assert_eq!(d.client_id, 1);
        assert_eq!(d.amount, dec!(10.0000));
        assert_eq!(d.status, DepositStatus::Applied);
    }
}
