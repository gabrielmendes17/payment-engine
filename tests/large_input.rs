//! Large-input functional tests. Verify that per-client invariants hold
//! across 100k deposits + 30k mixed dispute/resolve + 20k duplicate-tx
//! runs. Performance claims belong to a separate `cargo bench` target;
//! these tests only check correctness at scale.

use rust_decimal::Decimal;
use rust_decimal_macros::dec;

use payment_engine::adapters::outbound::InMemoryLedgerRepository;
use payment_engine::domain::Transaction;
use payment_engine::{ListAccounts, PaymentEngine, ProcessTransaction};

const DEPOSITS: u32 = 100_000;
const CLIENTS: u16 = 1_000;

/// Streams 100k deposits across 1k clients through the engine and verifies
/// every row applies and per-client totals match the expected sum.
/// Transactions are built one-at-a-time so no intermediate `Vec` is
/// allocated, exercising the streaming shape of the driver.
#[test]
fn hundred_thousand_deposits_apply_correctly() {
    let repo = InMemoryLedgerRepository::new();
    let mut engine = PaymentEngine::new(repo);

    for tx in 1..=DEPOSITS {
        let client = (tx % u32::from(CLIENTS)) as u16 + 1;
        engine
            .process(Transaction::Deposit {
                client,
                tx,
                amount: dec!(1.0000),
            })
            .unwrap();
    }

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
    let deposit_count = ROWS / 2;

    let repo = InMemoryLedgerRepository::new();
    let mut engine = PaymentEngine::new(repo);

    // First half: deposits so there is a pool of tx ids to dispute.
    for tx in 1..=deposit_count {
        let client = (tx % 100) as u16 + 1;
        engine
            .process(Transaction::Deposit {
                client,
                tx,
                amount: dec!(2.5000),
            })
            .unwrap();
    }
    // Second half: alternating dispute + resolve against the deposit pool.
    // Net effect on balances is zero, so per-client total stays at
    // deposit_count / clients * 2.5.
    for target_tx in 1..=(ROWS / 4) {
        let client = (target_tx % 100) as u16 + 1;
        engine
            .process(Transaction::Dispute {
                client,
                tx: target_tx,
            })
            .unwrap();
        engine
            .process(Transaction::Resolve {
                client,
                tx: target_tx,
            })
            .unwrap();
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

/// Confirms duplicate-tx detection stays correct at scale: the second
/// pass reuses every tx from a different client, so every reuse must be
/// rejected and no phantom account created for the impostor.
#[test]
fn duplicate_tx_detection_at_scale() {
    const APPLIED: u32 = 20_000;

    let repo = InMemoryLedgerRepository::new();
    let mut engine = PaymentEngine::new(repo);

    // First pass: applied deposits reserving tx 1..=APPLIED.
    for tx in 1..=APPLIED {
        engine
            .process(Transaction::Deposit {
                client: 1,
                tx,
                amount: dec!(1.0000),
            })
            .unwrap();
    }
    // Second pass: reuse every tx from a different client — all rejected.
    for tx in 1..=APPLIED {
        engine
            .process(Transaction::Deposit {
                client: 2,
                tx,
                amount: dec!(1.0000),
            })
            .unwrap();
    }

    let accounts = engine.list_accounts().unwrap();
    let client_1 = accounts.iter().find(|a| a.client_id() == 1).unwrap();
    assert_eq!(client_1.total(), Decimal::from(APPLIED));
    // Client 2 never had a successful deposit — every second-pass row was
    // rejected as a duplicate — so no account should have been created.
    assert!(accounts.iter().all(|a| a.client_id() != 2));
}
