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
