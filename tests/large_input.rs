//! Large-input regression test. Substantiates the O(n) claim in the README
//! and guards against accidental O(n²) regressions on the engine hot path
//! (deposit/withdraw/lifecycle dispatch and repository lookups). Tests build
//! `Transaction` values directly so the parser is not in the measurement.

use std::time::Instant;

use rust_decimal::Decimal;
use rust_decimal_macros::dec;

use payment_engine::adapters::outbound::InMemoryLedgerRepository;
use payment_engine::domain::Transaction;
use payment_engine::{ListAccounts, PaymentEngine, ProcessTransaction};

const DEPOSITS: u32 = 100_000;
const CLIENTS: u16 = 1_000;

/// Streams 100k deposits across 1k clients through the engine and verifies:
///
/// - every row is `Applied`;
/// - per-client totals match the expected sum;
/// - the run completes in well under a loose wall-clock bound (regressions
///   toward O(n²) would blow past this by orders of magnitude, not a few
///   percent, so the bound is deliberately generous).
///
/// Transactions are built directly to keep the CSV parser out of the timing.
#[test]
#[cfg_attr(debug_assertions, ignore = "run with --release for accurate timing")]
fn hundred_thousand_deposits_scale_linearly() {
    let mut transactions = Vec::with_capacity(DEPOSITS as usize);
    for tx in 1..=DEPOSITS {
        let client = (tx % u32::from(CLIENTS)) as u16 + 1;
        transactions.push(Transaction::Deposit {
            client,
            tx,
            amount: dec!(1.0000),
        });
    }

    let repo = InMemoryLedgerRepository::new();
    let mut engine = PaymentEngine::new(repo);

    let start = Instant::now();
    for transaction in transactions {
        engine.process(transaction).unwrap();
    }
    let elapsed = start.elapsed();

    // Generous ceiling; a linear run on modest hardware finishes well under
    // this. The point is to catch quadratic regressions, not to enforce a
    // real perf budget.
    assert!(
        elapsed.as_secs() < 30,
        "processing {DEPOSITS} deposits took {elapsed:?}, expected linear time"
    );

    let accounts = engine.list_accounts().unwrap();
    assert_eq!(accounts.len(), CLIENTS as usize);

    let per_client_rows = DEPOSITS / u32::from(CLIENTS);
    let expected_total: Decimal = Decimal::from(per_client_rows);
    for account in accounts {
        assert_eq!(
            account.total(),
            expected_total,
            "client {} total drifted",
            account.client_id()
        );
        assert_eq!(account.held(), Decimal::ZERO);
        assert!(!account.is_locked());
    }
}

/// Interleaves deposits with disputes and resolves against a shrinking
/// tail of prior transactions. Exercises the dispute-map lookup on the
/// hot path (not just the deposit insert path) and confirms per-client
/// invariants under mixed traffic at scale.
#[test]
fn interleaved_disputes_and_resolves_preserve_invariants_at_scale() {
    const ROWS: u32 = 30_000;

    // First half: deposits so there is a pool of tx ids to dispute.
    let deposit_count = ROWS / 2;
    let mut transactions: Vec<Transaction> = (1..=deposit_count)
        .map(|tx| Transaction::Deposit {
            client: (tx % 100) as u16 + 1,
            tx,
            amount: dec!(2.5000),
        })
        .collect();
    // Second half: alternating dispute + resolve against the deposit pool.
    // Net effect on balances is zero, so per-client total stays at
    // deposit_count / clients * 2.5.
    for target_tx in 1..=(ROWS / 4) {
        let client = (target_tx % 100) as u16 + 1;
        transactions.push(Transaction::Dispute {
            client,
            tx: target_tx,
        });
        transactions.push(Transaction::Resolve {
            client,
            tx: target_tx,
        });
    }

    let repo = InMemoryLedgerRepository::new();
    let mut engine = PaymentEngine::new(repo);
    for transaction in transactions {
        engine.process(transaction).unwrap();
    }
    let accounts = engine.list_accounts().unwrap();

    let deposits_per_client = deposit_count / 100;
    let expected_total = Decimal::from(deposits_per_client) * dec!(2.5);
    assert_eq!(accounts.len(), 100);
    for account in accounts {
        assert_eq!(account.held(), Decimal::ZERO);
        assert_eq!(account.available(), expected_total);
        assert_eq!(account.total(), expected_total);
        assert!(!account.is_locked());
    }
}

/// Confirms duplicate-tx detection stays O(1) per row at scale — a linear
/// scan on the seen-set would show up here.
#[test]
fn duplicate_tx_detection_scales() {
    const APPLIED: u32 = 20_000;

    // First pass: applied deposits reserving tx 1..=APPLIED.
    // Second pass: reuse every tx from a different client — all rejected.
    let mut transactions = Vec::with_capacity(APPLIED as usize * 2);
    for tx in 1..=APPLIED {
        transactions.push(Transaction::Deposit {
            client: 1,
            tx,
            amount: dec!(1.0000),
        });
    }
    for tx in 1..=APPLIED {
        transactions.push(Transaction::Deposit {
            client: 2,
            tx,
            amount: dec!(1.0000),
        });
    }

    let repo = InMemoryLedgerRepository::new();
    let mut engine = PaymentEngine::new(repo);

    let start = Instant::now();
    for transaction in transactions {
        engine.process(transaction).unwrap();
    }
    let elapsed = start.elapsed();

    assert!(
        elapsed.as_secs() < 30,
        "duplicate detection on {APPLIED} + {APPLIED} rows took {elapsed:?}"
    );

    let accounts = engine.list_accounts().unwrap();
    let client_1 = accounts.iter().find(|a| a.client_id() == 1).unwrap();
    assert_eq!(client_1.total(), Decimal::from(APPLIED));
    // Client 2 never had a successful deposit — every second-pass row was
    // rejected as a duplicate — so no account should have been created.
    assert!(accounts.iter().all(|a| a.client_id() != 2));
}
