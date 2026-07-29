use std::io::Write;

use rust_decimal::Decimal;
use serde::Serialize;

use crate::domain::Account;

// Precision policy: normalize every amount to exactly four fractional digits
// on the way out. The spec is lax about displayed precision, but consistent
// 4-decimal output round-trips cleanly with 4-decimal input.
const OUTPUT_SCALE: u32 = 4;

fn format_amount(value: Decimal) -> String {
    // round_dp trims trailing zeroes; format with {:.4} to zero-pad so every
    // amount is exactly four fractional digits.
    format!("{:.*}", OUTPUT_SCALE as usize, value.round_dp(OUTPUT_SCALE))
}

#[derive(Debug, Serialize)]
struct AccountRow {
    client: u16,
    available: String,
    held: String,
    total: String,
    locked: bool,
}

impl AccountRow {
    fn from_account(a: &Account) -> Self {
        Self {
            client: a.client_id(),
            available: format_amount(a.available()),
            held: format_amount(a.held()),
            total: format_amount(a.total()),
            locked: a.is_locked(),
        }
    }
}

/// Write the final account snapshot as CSV. Row order is not significant
/// per the spec, so we do not sort here — callers may sort before calling
/// if a stable order helps their tests.
pub fn write_accounts<W: Write>(writer: W, accounts: &[Account]) -> Result<(), ::csv::Error> {
    let mut csv_writer = ::csv::WriterBuilder::new()
        // Header is written explicitly below so we always emit it even when
        // `accounts` is empty. csv::Writer with has_headers=true only writes
        // the header on the first serialize() call.
        .has_headers(false)
        .from_writer(writer);
    csv_writer.write_record(["client", "available", "held", "total", "locked"])?;
    for account in accounts {
        csv_writer.serialize(AccountRow::from_account(account))?;
    }
    csv_writer.flush()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::DepositRecord;
    use crate::domain::services::dispute_service;
    use rust_decimal_macros::dec;

    fn parse_output(bytes: &[u8]) -> Vec<Vec<String>> {
        let mut rdr = ::csv::ReaderBuilder::new()
            .has_headers(false)
            .from_reader(bytes);
        rdr.records()
            .map(|r| r.unwrap().iter().map(|s| s.to_string()).collect())
            .collect()
    }

    fn credited(client: u16, amount: Decimal) -> Account {
        Account::new(client).credit(amount).unwrap()
    }

    // Builds an account with `available = -amount` and `held = 0` and `locked = true`
    // by running through deposit -> withdrawal -> dispute -> chargeback.
    // Mirrors the end-to-end scenario in specs/05-acceptance-scenarios.md.
    fn chargeback_locked_negative(client: u16, deposited: Decimal, withdrawn: Decimal) -> Account {
        let account = Account::new(client)
            .credit(deposited)
            .unwrap()
            .debit(withdrawn)
            .unwrap();
        let deposit = DepositRecord::new_applied(1, client, deposited);
        let (account, deposit) = dispute_service::apply_dispute(account, deposit).unwrap();
        let (account, _) = dispute_service::apply_chargeback(account, deposit).unwrap();
        account
    }

    #[test]
    fn writes_header_and_account_rows_with_four_decimal_precision() {
        let accounts = vec![credited(1, dec!(1.5)), credited(2, dec!(2))];
        let mut buf = Vec::new();
        write_accounts(&mut buf, &accounts).unwrap();
        let rows = parse_output(&buf);
        assert_eq!(
            rows[0],
            vec!["client", "available", "held", "total", "locked"]
        );
        assert_eq!(rows[1], vec!["1", "1.5000", "0.0000", "1.5000", "false"]);
        assert_eq!(rows[2], vec!["2", "2.0000", "0.0000", "2.0000", "false"]);
    }

    #[test]
    fn preserves_negative_available_and_locked_flag() {
        // Deposit 100, withdraw 70 (available=30), dispute+chargeback the
        // original 100 -> available=-70, held=0, locked=true, total=-70.
        // Matches the shape of the end-to-end fixture.
        let account = chargeback_locked_negative(1, dec!(100), dec!(70));
        let mut buf = Vec::new();
        write_accounts(&mut buf, &[account]).unwrap();
        let rows = parse_output(&buf);
        assert_eq!(rows[1], vec!["1", "-70.0000", "0.0000", "-70.0000", "true"]);
    }

    #[test]
    fn held_and_total_are_derived_consistently() {
        // Fund 3, then dispute a 10-value synthetic deposit -> available=-7,
        // held=10, total=3.
        let account = Account::new(5).credit(dec!(3)).unwrap();
        let deposit = DepositRecord::new_applied(1, 5, dec!(10));
        let (account, _) = dispute_service::apply_dispute(account, deposit).unwrap();
        let mut buf = Vec::new();
        write_accounts(&mut buf, &[account]).unwrap();
        let rows = parse_output(&buf);
        assert_eq!(rows[1], vec!["5", "-7.0000", "10.0000", "3.0000", "false"]);
    }

    #[test]
    fn empty_accounts_writes_only_header() {
        let mut buf = Vec::new();
        write_accounts(&mut buf, &[]).unwrap();
        let rows = parse_output(&buf);
        assert_eq!(rows.len(), 1);
        assert_eq!(
            rows[0],
            vec!["client", "available", "held", "total", "locked"]
        );
    }
}
