use std::collections::HashMap;
use std::path::PathBuf;
use std::process::{Command, Stdio};

fn fixture_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name)
}

fn binary_path() -> &'static str {
    env!("CARGO_BIN_EXE_payment-engine")
}

fn parse_rows(csv: &str) -> HashMap<u16, [String; 4]> {
    let mut rdr = ::csv::ReaderBuilder::new()
        .has_headers(true)
        .trim(::csv::Trim::All)
        .from_reader(csv.as_bytes());
    let mut out = HashMap::new();
    for record in rdr.records() {
        let r = record.unwrap();
        let client: u16 = r[0].parse().unwrap();
        out.insert(
            client,
            [
                r[1].to_string(),
                r[2].to_string(),
                r[3].to_string(),
                r[4].to_string(),
            ],
        );
    }
    out
}

#[test]
fn spec_sample_binary_prints_expected_csv_and_exits_zero() {
    let input = fixture_path("spec_sample.csv");

    let output = Command::new(binary_path())
        .arg(&input)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("spawn binary");

    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
    let stderr = String::from_utf8(output.stderr).expect("utf8 stderr");
    assert!(
        output.status.success(),
        "expected success, stderr: {stderr}"
    );
    let rows = parse_rows(&stdout);
    assert_eq!(rows.len(), 2);
    assert_eq!(
        rows[&1],
        [
            "1.5000".to_string(),
            "0.0000".to_string(),
            "1.5000".to_string(),
            "false".to_string(),
        ]
    );
    assert_eq!(
        rows[&2],
        [
            "2.0000".to_string(),
            "0.0000".to_string(),
            "2.0000".to_string(),
            "false".to_string(),
        ]
    );
    assert!(
        stderr.is_empty(),
        "stderr should be empty on happy path, got: {stderr:?}"
    );
}

#[test]
fn missing_input_file_exits_non_zero_with_error_on_stderr() {
    let output = Command::new(binary_path())
        .arg("/nonexistent/path/to/file.csv")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("spawn binary");

    assert!(
        !output.status.success(),
        "expected non-zero exit for missing file"
    );
    let stderr = String::from_utf8(output.stderr).expect("utf8 stderr");
    assert!(
        stderr.contains("error:") && stderr.contains("opening input file"),
        "stderr should describe the failure, got: {stderr:?}"
    );
}

#[test]
fn missing_argument_exits_non_zero_with_usage_on_stderr() {
    let output = Command::new(binary_path())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("spawn binary");

    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).expect("utf8 stderr");
    assert!(
        stderr.contains("usage: payment-engine"),
        "stderr should include usage, got: {stderr:?}"
    );
}
