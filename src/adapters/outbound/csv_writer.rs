use std::io::Write;

use rust_decimal::Decimal;
use serde::Serialize;

use crate::domain::Account;

const OUTPUT_SCALE: u32 = 4;

fn format_amount(value: Decimal) -> String {
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

pub fn write_accounts<W: Write>(writer: W, accounts: &[Account]) -> Result<(), ::csv::Error> {
    // Header is written explicitly so it's still emitted when `accounts` is
    // empty — `has_headers(true)` only emits on the first serialize call.
    let mut csv_writer = ::csv::WriterBuilder::new()
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
    use crate::domain::Deposit;
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

    fn chargeback_locked_negative(client: u16, deposited: Decimal, withdrawn: Decimal) -> Account {
        let account = Account::new(client)
            .credit(deposited)
            .unwrap()
            .debit(withdrawn)
            .unwrap();
        let deposit = Deposit::new(1, client, deposited).unwrap();
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
        let account = chargeback_locked_negative(1, dec!(100), dec!(70));
        let mut buf = Vec::new();
        write_accounts(&mut buf, &[account]).unwrap();
        let rows = parse_output(&buf);
        assert_eq!(rows[1], vec!["1", "-70.0000", "0.0000", "-70.0000", "true"]);
    }

    #[test]
    fn held_and_total_are_derived_consistently() {
        let account = Account::new(5).credit(dec!(3)).unwrap();
        let deposit = Deposit::new(1, 5, dec!(10)).unwrap();
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
