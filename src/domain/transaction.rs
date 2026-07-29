use rust_decimal::Decimal;

use crate::domain::account::ClientId;

pub type TransactionId = u32;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Transaction {
    Deposit {
        client: ClientId,
        tx: TransactionId,
        amount: Decimal,
    },
    Withdrawal {
        client: ClientId,
        tx: TransactionId,
        amount: Decimal,
    },
    Dispute {
        client: ClientId,
        tx: TransactionId,
    },
    Resolve {
        client: ClientId,
        tx: TransactionId,
    },
    Chargeback {
        client: ClientId,
        tx: TransactionId,
    },
}

impl Transaction {
    pub fn client(&self) -> ClientId {
        match self {
            Self::Deposit { client, .. }
            | Self::Withdrawal { client, .. }
            | Self::Dispute { client, .. }
            | Self::Resolve { client, .. }
            | Self::Chargeback { client, .. } => *client,
        }
    }

    pub fn tx(&self) -> TransactionId {
        match self {
            Self::Deposit { tx, .. }
            | Self::Withdrawal { tx, .. }
            | Self::Dispute { tx, .. }
            | Self::Resolve { tx, .. }
            | Self::Chargeback { tx, .. } => *tx,
        }
    }

    pub fn is_primary(&self) -> bool {
        matches!(self, Self::Deposit { .. } | Self::Withdrawal { .. })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn accessors_return_carried_client_and_tx() {
        let d = Transaction::Deposit {
            client: 1,
            tx: 10,
            amount: dec!(1.0000),
        };
        assert_eq!(d.client(), 1);
        assert_eq!(d.tx(), 10);
    }

    #[test]
    fn deposit_and_withdrawal_are_primary() {
        assert!(
            Transaction::Deposit {
                client: 1,
                tx: 1,
                amount: dec!(1)
            }
            .is_primary()
        );
        assert!(
            Transaction::Withdrawal {
                client: 1,
                tx: 2,
                amount: dec!(1)
            }
            .is_primary()
        );
    }

    #[test]
    fn lifecycle_events_are_not_primary() {
        assert!(!Transaction::Dispute { client: 1, tx: 1 }.is_primary());
        assert!(!Transaction::Resolve { client: 1, tx: 1 }.is_primary());
        assert!(!Transaction::Chargeback { client: 1, tx: 1 }.is_primary());
    }
}
