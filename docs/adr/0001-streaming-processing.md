# ADR 0001: Sequential Streaming Processing

## Status

Accepted

## Decision

Process one chronological CSV row at a time.

The in-memory repository maintains:

```text
accounts: HashMap<ClientId, Account>
seen_transaction_ids: HashSet<TransactionId>
deposits: HashMap<TransactionId, DepositRecord>
```

Only deposits retain full transaction details because only deposits participate in the dispute lifecycle.

## Consequences

Positive:

- preserves order;
- avoids locks and races;
- does not retain withdrawal details;
- provides defensive duplicate detection;
- streams input in linear time.

Negative:

- deposit IDs occur in both the seen-ID set and deposit map;
- the seen-ID set grows with every primary transaction;
- processing is not parallelized.

The duplicated deposit key is accepted because the set and map have different responsibilities.

## Alternatives

- Storing every primary transaction was rejected as unnecessary memory use.
- Storing only deposits was rejected because duplicate withdrawals would not be detected.
- A deposit map plus withdrawal-only set was rejected because duplicate checks would span two collections.
- Parallel workers were rejected because the required input is one ordered stream.
