# ADR 0003: Concurrency Model

## Status

Accepted

## Context

The challenge input is one chronologically ordered CSV stream. The hint also raises a hypothetical server ingesting thousands of concurrent TCP streams.

Two ordering guarantees must be preserved:

- Per-stream order. A dispute must observe its deposit; a chargeback must observe its dispute.
- Per-client order across streams, when clients can appear on more than one stream.

## Decision

### Within one stream

Process rows sequentially in one thread. Do not introduce a channel between the CSV reader and the application service.

Reasoning:

- Introducing a channel would add synchronization, buffering, and scheduling overhead without providing useful parallelism for the current ordered single-stream workload.
- Naively processing rows concurrently would violate per-stream ordering. Safe parallelization would require partitioning by `client_id` and keeping each partition sequential (see "Across many streams" below); it is unnecessary for the current single-stream scope.

The CSV adapter exposes an internal `process_transactions` helper that consumes any `IntoIterator<Item = Result<Transaction, CsvInputError>>`. This keeps the door open for a channel-fed source later without committing to a public source port now.

### Across many streams

The unit of parallelism is the stream, not the stage.

Two composable patterns are allowed at the composition root:

- One engine per stream when clients do not cross streams. Each `PaymentEngine` owns its `InMemoryLedgerRepository`. No shared state.
- A router in front of N engines, sharded by `client_id`, when clients can appear on any stream. The router uses one MPSC per shard. Each shard remains internally sequential. This preserves per-client ordering while giving N-way parallelism across clients.

### Shared storage

The current port is sufficient for sequential in-memory processing. Replacing the in-memory adapter with a database-backed `LedgerRepository` would preserve `commit` atomicity, and a unique constraint on `tx` would provide duplicate protection at the storage layer.

However, a *concurrent* database adapter cannot preserve the current semantics with the existing port alone. The engine currently reads `transaction_seen(tx)` and later calls `commit(changes)` as two independent operations. Under concurrency, two callers can both observe `transaction_seen(tx) == false` before either commits; a unique-constraint conflict at commit time then surfaces as `Err(Repository(err))` rather than the intended `Ok(ApplyOutcome::Rejected(RejectionReason::DuplicateTransaction))`. Account updates are similarly exposed to lost writes because the account snapshot is read outside the eventual database transaction.

Preserving today's guarantees under concurrency would therefore likely require one of: transactional read/write inside `commit` (SELECT ... FOR UPDATE), optimistic-version conflicts, an atomic reservation result returned from `commit`, or a typed duplicate-conflict variant on `Self::Error` the application maps to `RejectionReason::DuplicateTransaction`. That is a *port change*, not just an adapter swap. This ADR intentionally does not commit to a shape; the note exists so a future contributor doesn't ship a concurrent adapter under the false assumption that the port already handles it.

## Consequences

Positive:

- Preserves per-stream ordering by construction.
- Avoids channel overhead on the hot path.
- Scales horizontally across streams and, via sharding, across clients.
- Concurrency choices stay at the composition root.

Negative:

- Sharding by `client_id` is not implemented in this crate.
- Cross-stream client contention requires either a router or a shared repository adapter.

## Why `&mut self` on the ports

`ProcessTransaction::process` and `LedgerRepository::commit` both take `&mut self`. This is intentional. It prevents accidental shared mutation and forces the composition root to choose a concurrency posture. A future shared-state adapter can wrap its own synchronization internally without changing the port signatures.
