use std::env;
use std::fs::File;
use std::io::{self, BufReader, BufWriter, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::{Context, Result, anyhow};

use payment_engine::adapters::inbound::{drive, parse_rows};
use payment_engine::adapters::outbound::{InMemoryPaymentRepository, write_accounts};
use payment_engine::application::{PaymentEngine, PaymentRepository};

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            // Diagnostics go to stderr per specs/04-io-contract.md.
            let _ = writeln!(io::stderr(), "error: {err:#}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<()> {
    let input_path = parse_args()?;

    let file = File::open(&input_path)
        .with_context(|| format!("opening input file {}", input_path.display()))?;
    let reader = BufReader::new(file);

    let repository = InMemoryPaymentRepository::new();
    let mut engine = PaymentEngine::new(repository);

    drive(parse_rows(reader), &mut engine).context("processing transactions")?;

    let accounts = engine
        .repository()
        .accounts()
        .expect("in-memory repository is infallible");

    let stdout = io::stdout();
    let mut out = BufWriter::new(stdout.lock());
    write_accounts(&mut out, &accounts).context("writing accounts CSV")?;
    out.flush().context("flushing stdout")?;
    Ok(())
}

fn parse_args() -> Result<PathBuf> {
    let mut args = env::args_os().skip(1);
    let path = args
        .next()
        .ok_or_else(|| anyhow!("usage: payment-engine <transactions.csv>"))?;
    if args.next().is_some() {
        return Err(anyhow!(
            "unexpected extra arguments; usage: payment-engine <transactions.csv>"
        ));
    }
    Ok(PathBuf::from(path))
}
