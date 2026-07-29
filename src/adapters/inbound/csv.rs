use std::io::Read;

use rust_decimal::Decimal;
use serde::Deserialize;
use thiserror::Error;

use crate::application::ports::inbound::ProcessTransaction;
use crate::domain::{ClientId, Transaction, TransactionId};

#[derive(Debug, Error)]
pub enum AdapterError {
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

    #[error("processor error: {0}")]
    Processor(String),
}

// Wire format. `type` is a reserved keyword, so we rename via serde.
#[derive(Debug, Deserialize)]
struct CsvRow {
    #[serde(rename = "type")]
    kind: String,
    client: ClientId,
    tx: TransactionId,
    // Optional so dispute/resolve/chargeback rows (which have no amount)
    // parse correctly. We validate presence at conversion time.
    amount: Option<Decimal>,
}

impl CsvRow {
    fn into_transaction(self, row: u64) -> Result<Transaction, AdapterError> {
        // Trim so surrounding whitespace on the type column is tolerated.
        let kind = self.kind.trim();
        match kind {
            "deposit" => Ok(Transaction::Deposit {
                client: self.client,
                tx: self.tx,
                amount: self.amount.ok_or(AdapterError::MissingAmount {
                    row,
                    kind: "deposit",
                    tx: self.tx,
                })?,
            }),
            "withdrawal" => Ok(Transaction::Withdrawal {
                client: self.client,
                tx: self.tx,
                amount: self.amount.ok_or(AdapterError::MissingAmount {
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
            other => Err(AdapterError::UnknownType {
                row,
                value: other.to_string(),
            }),
        }
    }
}

/// Stream CSV rows from any `Read` source, one at a time. Each element is
/// either a parsed `Transaction` or an `AdapterError` describing where and
/// why parsing failed.
pub fn parse_rows<R: Read>(reader: R) -> impl Iterator<Item = Result<Transaction, AdapterError>> {
    let csv_reader = ::csv::ReaderBuilder::new()
        .has_headers(true)
        .trim(::csv::Trim::All) // tolerate whitespace around every field
        .flexible(true) // dispute/resolve/chargeback may omit the amount field entirely
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
    type Item = Result<Transaction, AdapterError>;

    fn next(&mut self) -> Option<Self::Item> {
        let raw = self.inner.next()?;
        self.row += 1;
        let row_number = self.row;
        Some(match raw {
            Ok(csv_row) => csv_row.into_transaction(row_number),
            Err(source) => Err(AdapterError::Csv {
                row: row_number,
                source,
            }),
        })
    }
}

/// Drive a stream of parsed transactions through an inbound port.
///
/// - Domain rejections from the port do not stop the stream.
/// - CSV parse errors and processor errors do stop the stream.
/// - This function is generic over the iterator so it can be fed by
///   `parse_rows`, an in-memory `Vec`, or (in the future) a channel-backed
///   source. See `docs/adr/0003-concurrency-model.md`.
pub fn drive<I, P>(source: I, port: &mut P) -> Result<(), AdapterError>
where
    I: IntoIterator<Item = Result<Transaction, AdapterError>>,
    P: ProcessTransaction,
    P::Error: std::fmt::Display,
{
    for item in source {
        let transaction = item?;
        // Ignore ApplyOutcome::Rejected — it is a normal domain event.
        port.process(transaction)
            .map_err(|e| AdapterError::Processor(e.to_string()))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{ApplyOutcome, Transaction};
    use rust_decimal_macros::dec;

    // Scenario 18: a valid CSV row converts into the correct domain transaction
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

    // Scenario 19: missing deposit or withdrawal amount fails before the application
    #[test]
    fn missing_deposit_or_withdrawal_amount_is_a_parse_error() {
        let csv = "\
type,client,tx,amount
deposit,1,1,
";
        let err = parse_rows(csv.as_bytes()).next().unwrap().unwrap_err();
        assert!(matches!(
            err,
            AdapterError::MissingAmount {
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
            AdapterError::MissingAmount {
                kind: "withdrawal",
                ..
            }
        ));
    }

    // Scenario 21: a malformed CSV row stops processing with an adapter error
    #[test]
    fn unknown_type_yields_adapter_error() {
        let csv = "\
type,client,tx,amount
teleport,1,1,1.0
";
        let err = parse_rows(csv.as_bytes()).next().unwrap().unwrap_err();
        assert!(matches!(err, AdapterError::UnknownType { .. }));
    }

    #[test]
    fn invalid_decimal_is_a_csv_error() {
        let csv = "\
type,client,tx,amount
deposit,1,1,not-a-number
";
        let err = parse_rows(csv.as_bytes()).next().unwrap().unwrap_err();
        assert!(matches!(err, AdapterError::Csv { .. }));
    }

    #[test]
    fn invalid_client_is_a_csv_error() {
        let csv = "\
type,client,tx,amount
deposit,999999,1,1.0
";
        let err = parse_rows(csv.as_bytes()).next().unwrap().unwrap_err();
        assert!(matches!(err, AdapterError::Csv { .. }));
    }

    // In-memory fake port for the driver tests
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

    #[derive(Debug)]
    struct PortErr(&'static str);
    impl std::fmt::Display for PortErr {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str(self.0)
        }
    }

    impl ProcessTransaction for RecordingPort {
        type Error = PortErr;
        fn process(&mut self, transaction: Transaction) -> Result<ApplyOutcome, Self::Error> {
            self.seen.push(transaction);
            if let Some(n) = self.fail_at {
                if self.seen.len() > n {
                    return Err(PortErr("boom"));
                }
            }
            // Always applied — domain-level rejection is out of scope for the driver.
            Ok(ApplyOutcome::Applied)
        }
    }

    // Scenario 20: a domain rejection does not stop the CSV stream
    #[test]
    fn drive_ignores_domain_rejections_and_processes_all_rows() {
        struct RejectingPort {
            count: usize,
        }
        impl ProcessTransaction for RejectingPort {
            type Error = std::convert::Infallible;
            fn process(&mut self, _t: Transaction) -> Result<ApplyOutcome, Self::Error> {
                self.count += 1;
                Ok(ApplyOutcome::Rejected(
                    crate::domain::RejectionReason::InvalidAmount,
                ))
            }
        }
        let csv = "\
type,client,tx,amount
deposit,1,1,1.0
deposit,1,2,2.0
deposit,1,3,3.0
";
        let mut port = RejectingPort { count: 0 };
        drive(parse_rows(csv.as_bytes()), &mut port).unwrap();
        assert_eq!(port.count, 3);
    }

    #[test]
    fn drive_stops_on_processor_error() {
        let csv = "\
type,client,tx,amount
deposit,1,1,1.0
deposit,1,2,2.0
deposit,1,3,3.0
";
        let mut port = RecordingPort::failing_after(1);
        let err = drive(parse_rows(csv.as_bytes()), &mut port).unwrap_err();
        assert!(matches!(err, AdapterError::Processor(_)));
        assert_eq!(port.seen.len(), 2); // stopped after the second row raised
    }

    #[test]
    fn drive_stops_on_parse_error_without_calling_port_for_that_row() {
        let csv = "\
type,client,tx,amount
deposit,1,1,1.0
teleport,1,2,1.0
deposit,1,3,3.0
";
        let mut port = RecordingPort::new();
        let err = drive(parse_rows(csv.as_bytes()), &mut port).unwrap_err();
        assert!(matches!(err, AdapterError::UnknownType { .. }));
        assert_eq!(port.seen.len(), 1);
    }
}
