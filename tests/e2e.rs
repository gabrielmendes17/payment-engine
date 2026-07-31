use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use rust_decimal::Decimal;
use rust_decimal_macros::dec;

use payment_engine::adapters::inbound::{parse_rows, process_transactions};
use payment_engine::adapters::outbound::{InMemoryLedgerRepository, write_accounts};
use payment_engine::{ListAccounts, PaymentEngine};

fn fixture_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name)
}

fn run_pipeline(input_path: PathBuf) -> String {
    let input =
        fs::read(&input_path).unwrap_or_else(|e| panic!("read {}: {e}", input_path.display()));
    run_pipeline_from_bytes(&input)
}

#[test]
fn end_to_end_dispute_and_chargeback_lock_account() {
    let out = run_pipeline(fixture_path("end_to_end.csv"));
    let rows = parse_by_client(&out);
    assert_eq!(rows.len(), 1);
    assert_eq!(
        rows[&1],
        Row {
            available: dec!(-30.0000),
            held: Decimal::ZERO,
            total: dec!(-30.0000),
            locked: true,
        }
    );
}

#[test]
fn spec_sample_matches_documented_output() {
    let out = run_pipeline(fixture_path("spec_sample.csv"));
    let rows = parse_by_client(&out);
    assert_eq!(rows.len(), 2);
    assert_eq!(
        rows[&1],
        Row {
            available: dec!(1.5000),
            held: Decimal::ZERO,
            total: dec!(1.5000),
            locked: false,
        }
    );
    assert_eq!(
        rows[&2],
        Row {
            available: dec!(2.0000),
            held: Decimal::ZERO,
            total: dec!(2.0000),
            locked: false,
        }
    );
}

fn run_pipeline_from_bytes(input: &[u8]) -> String {
    let repo = InMemoryLedgerRepository::new();
    let mut engine = PaymentEngine::new(repo);
    process_transactions(parse_rows(input), &mut engine).expect("driver failed");
    let accounts = engine.list_accounts().unwrap();
    let mut buf = Vec::new();
    write_accounts(&mut buf, &accounts).expect("write_accounts failed");
    String::from_utf8(buf).expect("utf8")
}

#[derive(Debug, PartialEq, Eq)]
struct Row {
    available: Decimal,
    held: Decimal,
    total: Decimal,
    locked: bool,
}

fn parse_by_client(csv: &str) -> HashMap<u16, Row> {
    let mut rdr = ::csv::ReaderBuilder::new()
        .has_headers(true)
        .trim(::csv::Trim::All)
        .from_reader(csv.as_bytes());
    let mut out = HashMap::new();
    for record in rdr.records() {
        let r = record.unwrap();
        let client: u16 = r[0].parse().unwrap();
        let available: Decimal = r[1].parse().unwrap();
        let held: Decimal = r[2].parse().unwrap();
        let total: Decimal = r[3].parse().unwrap();
        let locked: bool = r[4].parse().unwrap();
        out.insert(
            client,
            Row {
                available,
                held,
                total,
                locked,
            },
        );
    }
    out
}

#[test]
fn whitespace_and_unordered_ids_produce_correct_per_client_totals() {
    let input = "\
 type , client , tx , amount
 deposit , 3 , 30 , 3.0000
 deposit , 1 , 10 , 1.0000
 deposit , 2 , 20 , 2.0000
 deposit , 1 , 11 , 0.5000
";
    let out = run_pipeline_from_bytes(input.as_bytes());
    let rows = parse_by_client(&out);
    assert_eq!(
        rows[&1],
        Row {
            available: dec!(1.5000),
            held: Decimal::ZERO,
            total: dec!(1.5000),
            locked: false,
        }
    );
    assert_eq!(
        rows[&2],
        Row {
            available: dec!(2.0000),
            held: Decimal::ZERO,
            total: dec!(2.0000),
            locked: false,
        }
    );
    assert_eq!(
        rows[&3],
        Row {
            available: dec!(3.0000),
            held: Decimal::ZERO,
            total: dec!(3.0000),
            locked: false,
        }
    );
}

#[test]
fn four_decimal_precision_preserved_through_pipeline() {
    let input = "\
type,client,tx,amount
deposit,1,1,0.1001
deposit,1,2,0.2002
";
    let rows = parse_by_client(&run_pipeline_from_bytes(input.as_bytes()));
    assert_eq!(rows[&1].available, dec!(0.3003));
    assert_eq!(rows[&1].total, dec!(0.3003));
    assert_eq!(rows[&1].held, Decimal::ZERO);
    assert!(!rows[&1].locked);
}

#[test]
fn empty_amount_fields_on_lifecycle_rows_are_tolerated() {
    let input = "\
type,client,tx,amount
deposit,1,1,10.0000
dispute,1,1,
resolve,1,1,
";
    let rows = parse_by_client(&run_pipeline_from_bytes(input.as_bytes()));
    assert_eq!(rows[&1].available, dec!(10.0000));
    assert_eq!(rows[&1].held, Decimal::ZERO);
    assert_eq!(rows[&1].total, dec!(10.0000));
    assert!(!rows[&1].locked);
}
