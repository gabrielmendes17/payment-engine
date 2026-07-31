# Payment Engine

A single-crate Rust payment processor that consumes a CSV of transactions
(deposits, withdrawals, disputes, resolves, chargebacks) and emits the
resulting per-client account snapshot.

## Build and run

Requires Rust 1.85+ (edition 2024).

```bash
cargo build --release
cargo run --release -- tests/fixtures/spec_sample.csv > accounts.csv
```

Input path is a required positional argument. Output goes to stdout;
diagnostics go to stderr.

Exit codes:

- `0` — success. Rejected transactions still count as success; they are
  normal business outcomes and do not stop the stream.
- `1` — I/O, parse, or repository failure. The error is printed to stderr
  with an `error:` prefix and its full cause chain.

## Architecture

Hexagonal (ports and adapters):

```
adapters/inbound   ──▶  application::ports::inbound::{ProcessTransaction, ListAccounts}
                                    │
                                    ▼
                          application::PaymentEngine
                                    │
                                    ├──▶  domain (Account, Deposit, dispute_service)
                                    │
                                    ▼
                        application::ports::outbound::LedgerRepository
                                    │
                                    ▼
                       adapters/outbound (InMemoryLedgerRepository)
```

- **`domain/`** — pure business rules. `Account` and `Deposit` are the
  domain entities; balance and lifecycle transitions consume `self` and
  return an updated entity. A single use case submits changes to both
  entities together through one `LedgerChanges` commit.
  `services::dispute_service` coordinates dispute-lifecycle
  operations that touch both `Account` and `Deposit`.
- **`application/`** — thin use cases per transaction type + the
  `PaymentEngine` dispatcher. Kept private at the crate root; the
  caller-facing types (`PaymentEngine`, inbound ports, `EngineError`,
  `ApplyOutcome`, `RejectionReason`, `LedgerRepository`,
  `LedgerChanges`, `CommittedChanges`) are publicly re-exported.
  External adapters implement `LedgerRepository` and consume
  committed change-sets via `LedgerChanges::into_parts() ->
  CommittedChanges`.
- **`adapters/inbound/`** — CSV parser and driver
  (`parse_rows`, `process_transactions`).
- **`adapters/outbound/`** — in-memory `LedgerRepository` and CSV writer.
- **`src/main.rs`** — composition root: opens the file, wires the engine,
  streams input, writes output.

## Business assumptions

Decisions the implementation commits to. Where the spec is silent, each
decision is stated so a reviewer can audit it directly.

- **Reservation on rejection (`tx` is a one-shot identifier)**: a
  rejected deposit or withdrawal (invalid amount, locked account,
  insufficient funds) still reserves its `tx` so the number cannot be
  reused later within the same processing run. Durable idempotency
  across process restarts would require a persistent transaction store
  or a database uniqueness constraint on `tx`.
- **Duplicate transaction IDs**: once a `tx` has been reserved by any
  prior deposit or withdrawal (applied or rejected), a later deposit or
  withdrawal that reuses it is rejected with `DuplicateTransaction`.
  Lifecycle operations (dispute/resolve/chargeback) do not reserve `tx`
  because they reference an existing deposit rather than introducing a
  new primary identity.
- **The chargeback lock is a per-account kill-switch, not a per-deposit
  flag**: after a chargeback, every subsequent operation on that account
  is rejected — deposits, withdrawals, disputes, resolves, *and*
  chargebacks of a different disputed deposit belonging to the same
  account. A `ChargedBack` account signals fraud on the client, not on a
  specific `tx`; nothing else should move on that account without human
  review. The deposit's `Disputed → ChargedBack` transition is a
  separate, per-deposit terminal guard against double-executing the same
  chargeback.
- **Lifecycle events only apply to deposits**: dispute/resolve/chargeback
  target the deposit lifecycle. A dispute referencing a withdrawal `tx`
  (or any other non-deposit) is reported as `DepositNotFound`. Rationale:
  chargebacks reverse incoming funds; withdrawals have no held state to
  release.
- **`available` may go negative**: if the client has already spent the
  disputed funds, `dispute` moves `amount` into `held` even when
  `available < amount`. This preserves the invariant `total = available
  + held`.
- **Ownership guard**: dispute/resolve/chargeback require the referenced
  deposit to belong to the client on the transaction. A mismatch is
  reported as `ClientMismatch` (with the deposit's `owner_client` and
  the caller's `requesting_client`) and never touches the ledger.

## Error strategy

- **Domain errors** (`AccountError`, `DepositError`, `DisputeError`) are
  narrow — each names only the invariants its owning entity is responsible
  for. They implement `std::error::Error` via `thiserror`.
- **Business rejection outcomes** (`RejectionReason`) are cross-cutting:
  they carry the union of domain errors plus repository-wide concerns
  (`DuplicateTransaction`, `DepositNotFound`). They live inside
  `Ok(ApplyOutcome::Rejected(_))`, not `Err`, because a rejected
  transaction is a normal business event. The application layer maps
  domain errors into `RejectionReason` at the use-case boundary.
- **Engine errors** (`EngineError<E>`) wrap repository failures with their
  concrete `E` so callers can downcast or match. `E` is required to be
  `std::error::Error + Send + Sync + 'static`.
- **CSV/driver errors** (`CsvInputError`, `DriveError<E>`) preserve the
  original processor error type — no stringification.
- **Rejected transactions** are normal business outcomes: they never
  stop the stream. Only parse errors and repository failures do.

## Complexity

- Time: **O(n) amortized** in the number of input rows. Each row
  triggers a constant number of `HashMap`/`HashSet` operations
  (amortized O(1)) and at most one commit.
- Memory: **O(clients + deposits + primary_txs)**. Accounts are
  retained for every client whose deposit or withdrawal reaches
  processing, including clients whose first primary transaction is
  rejected. Invalid lifecycle events do not create accounts. Deposits
  are kept because dispute/resolve/chargeback reference them. Every
  primary transaction id (deposit or withdrawal, applied or rejected)
  is retained in a `HashSet` so it cannot be reused. Lifecycle events
  add no new memory beyond mutating the referenced deposit's status.

## Testing

Unit tests are colocated with each module; integration tests live in
`tests/`. The suite covers:

- successful and insufficient-funds withdrawals;
- duplicate transaction rejection (deposits and withdrawals);
- dispute, resolve, and chargeback transitions;
- invalid and repeated lifecycle transitions
  (double dispute, resolve/chargeback before dispute, chargeback of an
  already-charged-back deposit, re-dispute after resolve);
- cross-client ownership protection on dispute/resolve/chargeback;
- disputes after deposited funds have already been spent
  (`available` goes negative, invariant preserved);
- account behavior after locking — every subsequent operation is
  rejected, including chargebacks of a different disputed deposit;
- exact four-decimal arithmetic through the pipeline;
- the invariant `total = available + held` across mixed sequences;
- CSV parsing (whitespace, empty amounts, malformed rows, unknown
  types) and complete CLI behavior (exit codes, stdout/stderr,
  argument validation).

Because row ordering is unspecified, integration tests parse the
generated CSV and compare accounts by `client` — they never do raw
text diffs.

Run everything:

```bash
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
```

## Known trade-offs

- **In-memory only**: the reference adapter is
  `InMemoryLedgerRepository`, backed by two `HashMap`s and one
  `HashSet`. The outbound `LedgerRepository` port is public, so
  additional adapters (e.g. a database) can be implemented outside
  this crate. See the "Concurrent DB adapter" note below.
- **Row ordering is unspecified**, as allowed by the challenge.
  Accounts are stored in a `HashMap` for amortized O(1) access. If
  deterministic output becomes necessary, the account snapshot can be
  sorted by `client` at the serialization boundary without changing
  the repository structure. Consumers must key by `client`, not row
  index.
- **Async / concurrent evolutions**: several patterns for scaling
  beyond a single stream (single-owner MPSC, client-sharded workers,
  replicated scans, pre-sharded files, per-client actors) were
  considered but not implemented. See
  `docs/adr/0004-asynchronous-evolution.md` for the catalog and
  trade-offs, and `docs/adr/0003-concurrency-model.md` for the two
  recommended postures.
- **Concurrent DB adapter would need port changes**: today the engine
  reads `transaction_seen` and then commits in two steps. Under
  concurrency, that split races: a unique-constraint conflict would
  surface as a `Repository(err)` rather than the intended
  `Ok(Rejected(DuplicateTransaction))`, and account updates could be
  lost. A production database adapter would likely need transactional or
  optimistic-locking semantics — a port change, not just an adapter
  swap.
- **Single-threaded synchronous driver**: `process_transactions` consumes
  a `IntoIterator<Item = Result<Transaction, _>>` so it works for both
  file streams and any future channel-backed source, but there is no
  concurrency in the current implementation.
- **Amount precision**: input and output are normalized to 4 fractional
  digits via `rust_decimal::Decimal`. No floating-point arithmetic ever
  touches balances.
- **`RejectionReason` is unified** rather than split per operation. This
  keeps `ApplyOutcome::Rejected(_)` easy to pattern-match and avoids
  layered `From` conversions in callers.
