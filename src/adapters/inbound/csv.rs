use std::io::Read;

use rust_decimal::Decimal;
use serde::Deserialize;
use thiserror::Error;

use crate::application::ports::inbound::ProcessTransaction;
use crate::domain::{ClientId, Transaction, TransactionId};

#[derive(Debug, Error)]
pub enum CsvInputError {
    #[error("csv parse error at row {row}: {source}")]
    Csv {
        row: u64,
        #[source]
        source: ::csv::Error,
    },

    #[error("unknown transaction type at row {row}: {value}")]
    UnknownType { row: u64, value: String },

    #[error("missing amount for {kind} at row {row} (tx {tx})")]
    MissingAmount {
        row: u64,
        kind: &'static str,
        tx: TransactionId,
    },
}

#[derive(Debug, Error)]
pub enum DriveError<E>
where
    E: std::error::Error + 'static,
{
    #[error(transparent)]
    Input(#[from] CsvInputError),

    #[error("transaction processor failed")]
    Processor(#[source] E),
}

#[derive(Debug, Deserialize)]
struct CsvRow {
    #[serde(rename = "type")]
    kind: String,
    client: ClientId,
    tx: TransactionId,
    amount: Option<Decimal>,
}

impl CsvRow {
    fn into_transaction(self, row: u64) -> Result<Transaction, CsvInputError> {
        let kind = self.kind.trim();
        match kind {
            "deposit" => Ok(Transaction::Deposit {
                client: self.client,
                tx: self.tx,
                amount: self.amount.ok_or(CsvInputError::MissingAmount {
                    row,
                    kind: "deposit",
                    tx: self.tx,
                })?,
            }),
            "withdrawal" => Ok(Transaction::Withdrawal {
                client: self.client,
                tx: self.tx,
                amount: self.amount.ok_or(CsvInputError::MissingAmount {
                    row,
                    kind: "withdrawal",
                    tx: self.tx,
                })?,
            }),
            "dispute" => Ok(Transaction::Dispute {
                client: self.client,
                tx: self.tx,
            }),
            "resolve" => Ok(Transaction::Resolve {
                client: self.client,
                tx: self.tx,
            }),
            "chargeback" => Ok(Transaction::Chargeback {
                client: self.client,
                tx: self.tx,
            }),
            other => Err(CsvInputError::UnknownType {
                row,
                value: other.to_string(),
            }),
        }
    }
}

pub fn parse_rows<R: Read>(reader: R) -> impl Iterator<Item = Result<Transaction, CsvInputError>> {
    let csv_reader = ::csv::ReaderBuilder::new()
        .has_headers(true)
        .trim(::csv::Trim::All)
        .flexible(true)
        .from_reader(reader);
    RowIter {
        inner: csv_reader.into_deserialize::<CsvRow>(),
        row: 0,
    }
}

struct RowIter<R: Read> {
    inner: ::csv::DeserializeRecordsIntoIter<R, CsvRow>,
    row: u64,
}

impl<R: Read> Iterator for RowIter<R> {
    type Item = Result<Transaction, CsvInputError>;

    fn next(&mut self) -> Option<Self::Item> {
        let raw = self.inner.next()?;
        self.row += 1;
        let row_number = self.row;
        Some(match raw {
            Ok(csv_row) => csv_row.into_transaction(row_number),
            Err(source) => Err(CsvInputError::Csv {
                row: row_number,
                source,
            }),
        })
    }
}

/// Drive parsed transactions through an inbound port. Parse and processor
/// errors stop the stream; domain rejections do not.
pub fn process_transactions<I, P>(source: I, port: &mut P) -> Result<(), DriveError<P::Error>>
where
    I: IntoIterator<Item = Result<Transaction, CsvInputError>>,
    P: ProcessTransaction,
    P::Error: std::error::Error + 'static,
{
    for item in source {
        let transaction = item?;
        port.process(transaction).map_err(DriveError::Processor)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::outcome::{ApplyOutcome, RejectionReason};
    use crate::domain::Transaction;
    use rust_decimal_macros::dec;

    #[test]
    fn valid_rows_convert_to_domain_transactions() {
        let csv = "\
type,client,tx,amount
deposit,1,1,1.0
withdrawal,2,2,0.5000
dispute,1,1,
resolve,1,1,
chargeback,1,1,
";
        let parsed: Vec<_> = parse_rows(csv.as_bytes())
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(
            parsed,
            vec![
                Transaction::Deposit {
                    client: 1,
                    tx: 1,
                    amount: dec!(1.0)
                },
                Transaction::Withdrawal {
                    client: 2,
                    tx: 2,
                    amount: dec!(0.5000)
                },
                Transaction::Dispute { client: 1, tx: 1 },
                Transaction::Resolve { client: 1, tx: 1 },
                Transaction::Chargeback { client: 1, tx: 1 },
            ]
        );
    }

    #[test]
    fn surrounding_whitespace_is_tolerated() {
        let csv = "\
type, client, tx, amount
 deposit , 1 , 1 , 1.2345
";
        let parsed: Vec<_> = parse_rows(csv.as_bytes())
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(
            parsed,
            vec![Transaction::Deposit {
                client: 1,
                tx: 1,
                amount: dec!(1.2345),
            }]
        );
    }

    #[test]
    fn four_decimal_precision_is_preserved() {
        let csv = "\
type,client,tx,amount
deposit,1,1,1.2345
";
        let parsed = parse_rows(csv.as_bytes()).next().unwrap().unwrap();
        match parsed {
            Transaction::Deposit { amount, .. } => assert_eq!(amount, dec!(1.2345)),
            _ => panic!("expected deposit"),
        }
    }

    #[test]
    fn missing_deposit_or_withdrawal_amount_is_a_parse_error() {
        let csv = "\
type,client,tx,amount
deposit,1,1,
";
        let err = parse_rows(csv.as_bytes()).next().unwrap().unwrap_err();
        assert!(matches!(
            err,
            CsvInputError::MissingAmount {
                kind: "deposit",
                ..
            }
        ));

        let csv = "\
type,client,tx,amount
withdrawal,1,1,
";
        let err = parse_rows(csv.as_bytes()).next().unwrap().unwrap_err();
        assert!(matches!(
            err,
            CsvInputError::MissingAmount {
                kind: "withdrawal",
                ..
            }
        ));
    }

    #[test]
    fn unknown_type_yields_adapter_error() {
        let csv = "\
type,client,tx,amount
teleport,1,1,1.0
";
        let err = parse_rows(csv.as_bytes()).next().unwrap().unwrap_err();
        assert!(matches!(err, CsvInputError::UnknownType { .. }));
    }

    #[test]
    fn invalid_decimal_is_a_csv_error() {
        let csv = "\
type,client,tx,amount
deposit,1,1,not-a-number
";
        let err = parse_rows(csv.as_bytes()).next().unwrap().unwrap_err();
        assert!(matches!(err, CsvInputError::Csv { .. }));
    }

    #[test]
    fn invalid_client_is_a_csv_error() {
        let csv = "\
type,client,tx,amount
deposit,999999,1,1.0
";
        let err = parse_rows(csv.as_bytes()).next().unwrap().unwrap_err();
        assert!(matches!(err, CsvInputError::Csv { .. }));
    }

    struct RecordingPort {
        seen: Vec<Transaction>,
        fail_at: Option<usize>,
    }

    impl RecordingPort {
        fn new() -> Self {
            Self {
                seen: vec![],
                fail_at: None,
            }
        }
        fn failing_after(n: usize) -> Self {
            Self {
                seen: vec![],
                fail_at: Some(n),
            }
        }
    }

    #[derive(Debug, thiserror::Error)]
    #[error("{0}")]
    struct PortErr(&'static str);

    impl ProcessTransaction for RecordingPort {
        type Error = PortErr;
        fn process(&mut self, transaction: Transaction) -> Result<ApplyOutcome, Self::Error> {
            self.seen.push(transaction);
            if let Some(n) = self.fail_at {
                if self.seen.len() > n {
                    return Err(PortErr("boom"));
                }
            }
            Ok(ApplyOutcome::Applied)
        }
    }

    #[test]
    fn process_transactions_ignores_domain_rejections_and_processes_all_rows() {
        #[derive(Debug, thiserror::Error)]
        enum Never {}
        struct RejectingPort {
            count: usize,
        }
        impl ProcessTransaction for RejectingPort {
            type Error = Never;
            fn process(&mut self, _t: Transaction) -> Result<ApplyOutcome, Self::Error> {
                self.count += 1;
                Ok(ApplyOutcome::Rejected(RejectionReason::InvalidAmount))
            }
        }
        let csv = "\
type,client,tx,amount
deposit,1,1,1.0
deposit,1,2,2.0
deposit,1,3,3.0
";
        let mut port = RejectingPort { count: 0 };
        process_transactions(parse_rows(csv.as_bytes()), &mut port).unwrap();
        assert_eq!(port.count, 3);
    }

    #[test]
    fn process_transactions_stops_on_processor_error_and_preserves_type() {
        let csv = "\
type,client,tx,amount
deposit,1,1,1.0
deposit,1,2,2.0
deposit,1,3,3.0
";
        let mut port = RecordingPort::failing_after(1);
        let err = process_transactions(parse_rows(csv.as_bytes()), &mut port).unwrap_err();
        match err {
            DriveError::Processor(PortErr(msg)) => assert_eq!(msg, "boom"),
            other => panic!("expected Processor error, got {other:?}"),
        }
        assert_eq!(port.seen.len(), 2);
    }

    #[test]
    fn process_transactions_stops_on_parse_error_without_calling_port_for_that_row() {
        let csv = "\
type,client,tx,amount
deposit,1,1,1.0
teleport,1,2,1.0
deposit,1,3,3.0
";
        let mut port = RecordingPort::new();
        let err = process_transactions(parse_rows(csv.as_bytes()), &mut port).unwrap_err();
        assert!(matches!(
            err,
            DriveError::Input(CsvInputError::UnknownType { .. })
        ));
        assert_eq!(port.seen.len(), 1);
    }
}
