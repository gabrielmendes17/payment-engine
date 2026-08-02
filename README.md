# Payment Engine

A single-crate Rust payment processor that consumes a CSV of transactions
(deposits, withdrawals, disputes, resolves, chargebacks) and emits the
resulting per-client account snapshot.

## Build and run

Requires Rust 1.85+ (edition 2024).

```bash
cargo build --release
cargo run --release -- tests/fixtures/spec_sample.csv > accounts.csv

# Debug build for local experiments
cargo run -- sample.csv
```

Input path is a required positional argument. Output goes to stdout;
diagnostics go to stderr.

Exit codes:

- `0` — success. Rejected transactions still count as success; they are
  normal business outcomes and do not stop the stream.
- `1` — I/O, parse, repository, or invariant failure (including balance
  arithmetic overflow). The error is printed to stderr with an `error:`
  prefix and its full cause chain.

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

## Future concurrent TCP ingestion

The submitted application intentionally keeps transaction processing
single-threaded. The CSV is already an ordered stream, and deposits,
withdrawals, disputes, resolves, and chargebacks are order-dependent. Running
rows concurrently would therefore require an explicit ordering policy and a
clear owner for mutable ledger state, while providing no clear benefit for the
current CLI.

If the application evolved into a service receiving transactions from multiple
TCP connections, network concurrency would be separated from ledger mutation.
Each connection could be handled by an asynchronous task responsible for
framing, deserialization, validation, and writing the response. Validated
commands would be sent through a **bounded MPSC channel** to one task that
exclusively owns the `PaymentEngine`:

```text
TCP connection tasks
        │
        ▼
bounded MPSC command channel
        │
        ▼
single PaymentEngine owner
        │
        ▼
LedgerRepository
```

This is an actor-style ownership model implemented with ordinary async tasks
and channels rather than an actor framework. The engine task owns all mutable
account, deposit, and transaction-ID state, and other tasks interact with it
only by sending commands. The existing domain and application code can remain
synchronous because the asynchronous behavior stays in the inbound adapter.
A one-shot response channel could return each `ApplyOutcome` or engine error to
the connection task.

A bounded channel is intentional. If producers submit transactions faster than
the engine can process them, the queue eventually fills and applies
backpressure to the connection tasks. An unbounded queue could continue growing
until memory is exhausted.

A shared `Arc<Mutex<Ledger>>` could prevent data races, but it would not by
itself define the business order of requests arriving through different
connections. Locking individual repository methods would also be insufficient:
the current use cases perform a read–compute–commit sequence, such as checking
`transaction_seen`, loading an account, computing a new state, and committing
the changes. The complete sequence must be atomic. A single engine-owning task
serializes the entire `PaymentEngine::process` operation and avoids holding
locks across asynchronous work.

If profiling later showed that one engine task was a throughput bottleneck, the
next step would be a fixed number of client shards:

```text
TCP connection tasks
        │
        ▼
router by client ID
        │
        ├── bounded queue ──▶ shard 0 PaymentEngine
        ├── bounded queue ──▶ shard 1 PaymentEngine
        └── bounded queue ──▶ shard N PaymentEngine
```

Every command for one client must always be routed to the same shard. This
preserves sequential processing for one account while allowing unrelated
clients to be processed in parallel. Shards should represent stable logical
partitions, not TCP connections, because clients may disconnect, reconnect, or
submit commands through different connections.

Sharding introduces coordination requirements that do not exist in the current
single-owner implementation. The current engine enforces globally unique
primary transaction IDs and can distinguish an unknown deposit from one owned
by another client. Independent shard repositories would need a global
transaction-ID reservation and ownership index, or persistent storage with
corresponding uniqueness and lookup guarantees, to preserve those semantics.

The service would also need an explicit ordering and idempotency contract. A
basic version could define the authoritative order for one client as the order
in which the router admits commands to that client's shard queue. A stronger
protocol could include a unique request ID and a monotonically increasing
per-client sequence number so retries, duplicates, gaps, and out-of-order
requests can be handled deterministically.

Channels and actors solve in-process concurrency, not durability. A production
service would additionally require persistent idempotency records, atomic
storage updates, restart recovery, shard ownership, graceful shutdown,
observability, and overload handling. These concerns are intentionally outside
the scope of this CLI, while the `ProcessTransaction` inbound port keeps the
core engine independent from CSV, TCP, Tokio, or any other transport.

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
- **Business rejections** (`RejectionReason`) live inside
  `Ok(ApplyOutcome::Rejected(_))`. Each use case classifies its domain
  error explicitly via `helpers::classify_account_error` /
  `helpers::classify_dispute_error`: business variants map to a
  `RejectionReason`; `AccountError::ArithmeticOverflow` is lifted to
  `EngineError::ArithmeticOverflow { client }` and terminates processing
  rather than downgrading to a rejection. There is intentionally no
  blanket `From<AccountError>` / `From<DisputeError>` because that
  conversion cannot represent the fatal variant safely.
- **Checked balance arithmetic**: every `Account` balance operation uses
  `checked_add` / `checked_sub`, computes all new field values before any
  assignment, and validates the resulting `available + held` so `total()`
  cannot panic on already-persisted state. Overflow returns a typed error
  rather than panicking.
- **Engine errors** (`EngineError<E>`) wrap repository failures with their
  concrete `E` so callers can downcast or match. `E` is required to be
  `std::error::Error + Send + Sync + 'static`. `ArithmeticOverflow` is a
  peer variant that carries the affected `client`.
- **CSV/driver errors** (`CsvInputError`, `DriveError<E>`) preserve the
  original processor error type — no stringification.
- **Rejected transactions** are normal business outcomes: they never stop
  the stream. Parse errors, repository failures, and arithmetic-overflow
  invariants do.

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
- decimal balance-arithmetic overflow (direct `checked_add` overflow,
  the combined `available + held` guard, and no repository commit on
  failure);
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
- **Async / concurrent evolutions are documented, not implemented**:
  the first recommended TCP evolution is a bounded MPSC channel feeding a
  single task that owns the complete `PaymentEngine`. Client-based sharding is
  a later optimization only if profiling demonstrates that the single owner is
  a bottleneck. See the dedicated section above,
  `docs/adr/0004-asynchronous-evolution.md`, and
  `docs/adr/0003-concurrency-model.md`.
- **Concurrent DB adapter would need port changes**: today the engine
  reads `transaction_seen` and then commits in two steps. Under
  concurrency, that split races: a unique-constraint conflict would
  surface as a `Repository(err)` rather than the intended
  `Ok(Rejected(DuplicateTransaction))`, and account updates could be
  lost. A production database adapter would likely need transactional or
  optimistic-locking semantics — a port change, not just an adapter
  swap.
- **Single-threaded synchronous driver**: `process_transactions` consumes
  an `IntoIterator<Item = Result<Transaction, _>>`, which is appropriate for
  CSV and other synchronous sources. A future Tokio MPSC adapter would use an
  asynchronous receive loop and call the same `ProcessTransaction` port; the
  current helper would not need to pretend that an async receiver is an
  iterator.
- **Amount precision**: input and output are normalized to 4 fractional
  digits via `rust_decimal::Decimal`. No floating-point arithmetic ever
  touches balances.
- **`RejectionReason` is a unified enum** rather than split per operation.
  Callers still get a single `ApplyOutcome::Rejected(_)` to match; the
  mapping from domain errors is done by explicit classifiers so fatal
  variants (arithmetic overflow) cannot silently downgrade to a
  rejection.
