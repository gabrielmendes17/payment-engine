# ADR 0004: Asynchronous Evolution Options

## Status

Accepted

## Context

ADR 0003 records the concurrency posture for the submitted CLI (sequential single-stream) and names two patterns for a future server (single-owner and client-sharded). This ADR catalogs the wider option space we considered, why we discarded some, and where each pattern would fit if the crate ever ships as a server rather than a CLI.

The domain and use cases are synchronous by design. Every option below sits *outside* those layers, in the runtime/adapters layer.

## Decision

Five patterns were considered. The two we recommend are listed in ADR 0003; the other three are recorded here so a future contributor understands why they weren't picked.

### 1. Many producers → one bounded MPSC → single engine owner

*Correctness-first server posture.*

```
TCP/file reader tasks
        │
        │ parsed Transaction
        ▼
bounded tokio::sync::mpsc
        │
        ▼
single PaymentEngine owner
```

Each async task reads and parses its input, then sends domain transactions to one consumer that owns all ledger state.

```rust
let (sender, mut receiver) = tokio::sync::mpsc::channel(1_000);

while let Some(transaction) = receiver.recv().await {
    engine.process(transaction)?;
}
```

Trade-offs:

- No locks around accounts or deposits — one authoritative state owner.
- Straightforward invariants and atomicity.
- Bounded channel provides backpressure without changing the domain layer.
- Domain engine stays synchronous.
- All transactions are eventually applied by one worker: throughput ceiling equal to that worker's rate.

### 2. Client-sharded workers with one MPSC per shard

*Higher-throughput production posture.*

```
input readers
     │
     ▼
client router
     │
     ├── shard 0 mpsc → worker 0
     ├── shard 1 mpsc → worker 1
     └── shard N mpsc → worker N
```

Every transaction for the same client always reaches the same worker:

```rust
let shard = usize::from(client_id) % worker_count;
senders[shard].send(transaction).await?;
```

**Sub-variant — direct ingress routing.** The router task can be eliminated by letting each reader own the full array of shard senders and calculate the shard itself. It skips one hop but requires every reader to see every sender.

Trade-offs:

- Sequential processing per client; parallelism across different clients.
- No account-level locks — each worker owns its repository partition.
- Bounded queues and backpressure per shard.
- Hot clients or regions can create hot shards.
- Changing the worker count remaps clients — needs care for stateful storage.
- Cross-stream event ordering (when the same client appears on multiple TCP streams) still needs a defined policy.

### 3. Replicated scans with client filtering — *rejected*

For multiple immutable CSV files: have every worker read every file and keep only rows for its shard.

```
worker 0 reads all files → keeps shard-0 clients
worker 1 reads all files → keeps shard-1 clients
worker 2 reads all files → keeps shard-2 clients
```

Trade-offs:

- Shared-nothing workers; no router; no channels required for the transaction data.
- Chronological order inside each file is preserved.
- `W workers × N rows = W × N` rows parsed. Every worker repeats the I/O, CSV parsing, and filtering.
- Not suitable for live TCP streams.

Viable only when state processing dominates parsing — the opposite of the current workload profile.

### 4. Upstream pre-sharded files

Partition transactions by client at ingestion, so each worker reads only its own files.

```
input stage
    │
    ├── clients for shard 0 → file 0
    ├── clients for shard 1 → file 1
    └── clients for shard N → file N
```

Trade-offs:

- Each transaction is read exactly once.
- Shared-nothing processing, no router.
- Requires control over the upstream ingestion or partitioning stage — external contract.

The better batch alternative to option 3 when the ingestion side can be co-designed.

### 5. Per-client actors — *rejected*

One actor/mailbox per client, each serializing operations for its client.

```
client 1 → actor/mailbox 1
client 2 → actor/mailbox 2
client 3 → actor/mailbox 3
```

Trade-offs:

- Natural per-client ordering; strong isolation; parallelism across clients.
- Potentially huge numbers of actors and mailboxes (one per active client).
- Actor lifecycle, eviction, restart, and persistence add operational complexity.
- Harder to operate than fixed shards, which give the same ordering property with a bounded number of workers.

Fixed shards (option 2) are simpler than one actor per client for this problem shape.

## Recommendation

For the submitted CLI:

```
CSV iterator → synchronous PaymentEngine → in-memory repository
```

For a correctness-first server:

```
many async readers → bounded MPSC → one engine owner
```

For a scalable production server:

```
many async readers → bounded client-sharded MPSC queues
                         → one engine/repository owner per shard
```

## Consequences

Positive:

- All five options preserve the sync-domain invariant: `tokio`, MPSC channels, retries, and backpressure sit outside the domain and use cases.
- The two recommended options (single-owner, client-sharded) are composable at the composition root; the second is an in-place upgrade of the first once cross-client contention becomes the bottleneck.
- Rejected options are documented, so a future contributor doesn't rediscover the "every worker reads every file" idea and wonder why the code doesn't do it.

Negative:

- None of the async patterns are implemented in this crate. Any adoption is a future effort with its own ADR.
- Cross-stream client ordering (option 2 with multiple TCP inputs) is left as a policy the composition root must define.

## Anchor

The important architectural decision is to keep the domain and use cases synchronous. `tokio`, MPSC channels, retries, and backpressure belong outside them, in the runtime/adapters layer.
