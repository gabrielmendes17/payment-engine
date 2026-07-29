use rust_decimal::Decimal;

pub type ClientId = u16;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Account {
    pub client_id: ClientId,
    pub available: Decimal,
    pub held: Decimal,
    pub locked: bool,
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

    pub fn total(&self) -> Decimal {
        self.available + self.held
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn new_account_starts_at_zero_and_unlocked() {
        let account = Account::new(42);
        assert_eq!(account.client_id, 42);
        assert_eq!(account.available, Decimal::ZERO);
        assert_eq!(account.held, Decimal::ZERO);
        assert_eq!(account.total(), Decimal::ZERO);
        assert!(!account.locked);
    }

    #[test]
    fn total_is_derived_from_available_plus_held() {
        let account = Account {
            client_id: 1,
            available: dec!(10.1234),
            held: dec!(2.5000),
            locked: false,
        };
        assert_eq!(account.total(), dec!(12.6234));
    }

    #[test]
    fn total_supports_negative_available_from_dispute() {
        // Scenario 12: a dispute after prior spending may make available negative
        // while held remains non-negative. total = available + held must still hold.
        let account = Account {
            client_id: 1,
            available: dec!(-5.0000),
            held: dec!(10.0000),
            locked: false,
        };
        assert_eq!(account.total(), dec!(5.0000));
    }
}
