use std::fs;
use std::path::PathBuf;

use payment_engine::adapters::inbound::{drive, parse_rows};
use payment_engine::adapters::outbound::{InMemoryPaymentRepository, write_accounts};
use payment_engine::application::{PaymentEngine, PaymentRepository};

fn fixture_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name)
}

fn run_pipeline(input_path: PathBuf) -> String {
    let input =
        fs::read(&input_path).unwrap_or_else(|e| panic!("read {}: {e}", input_path.display()));
    let repo = InMemoryPaymentRepository::new();
    let mut engine = PaymentEngine::new(repo);
    drive(parse_rows(input.as_slice()), &mut engine).expect("drive failed");
    let mut accounts = engine.repository().accounts().unwrap();
    // Sort so tests can compare bytes without depending on HashMap iteration
    // order. The spec says output order does not matter.
    accounts.sort_by_key(|a| a.client_id());
    let mut buf = Vec::new();
    write_accounts(&mut buf, &accounts).expect("write_accounts failed");
    String::from_utf8(buf).expect("utf8")
}

fn assert_matches_expected(actual: &str, expected_path: PathBuf) {
    let expected = fs::read_to_string(&expected_path)
        .unwrap_or_else(|e| panic!("read {}: {e}", expected_path.display()));
    // Normalize trailing newlines / line endings.
    let normalize = |s: &str| -> String { s.trim_end().replace("\r\n", "\n") };
    assert_eq!(normalize(actual), normalize(&expected), "unexpected output");
}

#[test]
fn end_to_end_dispute_and_chargeback_lock_account() {
    let out = run_pipeline(fixture_path("end_to_end.csv"));
    assert_matches_expected(&out, fixture_path("end_to_end_expected.csv"));
}

#[test]
fn spec_sample_matches_documented_output() {
    let out = run_pipeline(fixture_path("spec_sample.csv"));
    assert_matches_expected(&out, fixture_path("spec_sample_expected.csv"));
}
