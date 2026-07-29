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

- CSV parsing and one application call are both on the order of microseconds.
- A bounded MPSC channel adds an allocation and an atomic per row, plus a park or unpark round-trip.
- The processor cannot be parallelized without breaking per-stream ordering.
- The dominant cost is I/O, addressed by `BufReader` capacity, not by a channel.

The CSV adapter exposes an internal `drive` helper that consumes any `Iterator<Item = Result<Transaction, _>>`. This keeps the door open for a channel-fed source later without committing to a public source port now.

### Across many streams

The unit of parallelism is the stream, not the stage.

Two composable patterns are allowed at the composition root:

- One engine per stream when clients do not cross streams. Each `PaymentEngine` owns its `InMemoryPaymentRepository`. No shared state.
- A router in front of N engines, sharded by `client_id`, when clients can appear on any stream. The router uses one MPSC per shard. Each shard remains internally sequential. This preserves per-client ordering while giving N-way parallelism across clients.

### Shared storage

If cross-stream state must be shared, replace the in-memory adapter with a database-backed `PaymentRepository`. `commit` becomes one database transaction. A unique constraint on `tx` provides duplicate protection at the storage layer. No change to ports, domain, or application.

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

`ProcessTransaction::process` and `PaymentRepository::commit` both take `&mut self`. This is intentional. It prevents accidental shared mutation and forces the composition root to choose a concurrency posture. A future shared-state adapter can wrap its own synchronization internally without changing the port signatures.
