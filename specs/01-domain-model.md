# Domain Model

## Account

Each client has one account.

```text
Account {
    client_id: u16
    available: Decimal
    held: Decimal
    locked: bool
}
```

`total` is derived:

```text
total = available + held
```

## Identifiers

```text
ClientId      = u16
TransactionId = u32
```

Input order is authoritative and chronological.

Deposit and withdrawal rows introduce primary financial transactions. Their `tx` values are globally unique.

Dispute, resolve, and chargeback rows reference an existing deposit using the original `tx`. They do not create new transaction identities.

## Retained domain state

The in-memory repository stores:

```text
accounts: HashMap<ClientId, Account>
seen_transaction_ids: HashSet<TransactionId>
deposits: HashMap<TransactionId, DepositRecord>
```

### Accounts

Stores current balance and lock state for each client.

### Seen transaction identifiers

Stores every deposit and withdrawal `tx` observed by the engine.

This collection exists only for primary transaction idempotency. A primary identifier remains reserved even when its operation is rejected by a domain rule. Lifecycle rows are never inserted into this set.

### Deposits

Stores only deposits because later lifecycle events need the original client, amount, and status.

```text
DepositRecord {
    transaction_id: TransactionId
    client_id: ClientId
    amount: Decimal
    status: DepositStatus
}
```

Withdrawals are not retained after processing because no later operation needs their details.

## Deposit lifecycle

```text
DepositStatus {
    Applied
    Disputed
    ChargedBack
}
```

```text
Applied
  └── dispute ──> Disputed
                    ├── resolve ──> Applied
                    └── chargeback ──> ChargedBack
```

Invalid transitions leave all state unchanged.

## Idempotency model

### Primary transactions

For a deposit or withdrawal:

```text
if seen_transaction_ids already contains tx:
    reject as DuplicateTransaction
else:
    insert tx into seen_transaction_ids
    evaluate the business operation
```

### Lifecycle events

Dispute, resolve, and chargeback do not use `(type, tx)` as an idempotency key. Their validity depends on deposit status.

### Event-level limitation

The input has no independent lifecycle event identifier. After a resolve, the system cannot distinguish a legitimate later dispute from a delayed duplicate of the earlier dispute.

A production system would normally include an `event_id`, partner reference, or explicit idempotency key.

## Invariants

```text
total == available + held
held >= 0
```

Additional invariants:

- Every deposit key also exists in `seen_transaction_ids`.
- Withdrawals never require retained transaction records.
- Resolve and chargeback apply only to disputed deposits.
- Chargeback locks the owning account.
- Rejected lifecycle events leave all state unchanged.
- Rejected primary events reserve `tx` but change no balances.
- A dispute may make `available` negative.
