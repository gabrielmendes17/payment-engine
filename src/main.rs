use std::env;
use std::fs::File;
use std::io::{self, BufReader, BufWriter, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::{Context, Result, anyhow};

use payment_engine::adapters::inbound::{parse_rows, process_transactions};
use payment_engine::adapters::outbound::{InMemoryLedgerRepository, write_accounts};
use payment_engine::{ListAccounts, PaymentEngine};

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
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

    let repository = InMemoryLedgerRepository::new();
    let mut engine = PaymentEngine::new(repository);

    process_transactions(parse_rows(reader), &mut engine).context("processing transactions")?;

    let accounts = engine.list_accounts().context("listing accounts")?;

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
