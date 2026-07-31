# Payment Engine Specifications

These files define the behavior and architecture of the payment engine independently from its Rust implementation.

## Reading order

1. `01-domain-model.md` — accounts, deposits, transaction identity, balances, and invariants
2. `02-processing-rules.md` — preconditions and effects for every operation
3. `03-rejections-and-errors.md` — domain rejections, application errors, and adapter errors
4. `04-io-contract.md` — command-line and CSV contract
5. `05-acceptance-scenarios.md` — executable examples to convert into tests
6. `06-architecture-and-ports.md` — dependency boundaries, ports, adapters, and composition

Architecture decisions are recorded under `docs/adr/`.

## Chosen state model

The in-memory adapter stores:

```text
accounts: HashMap<ClientId, Account>
seen_transaction_ids: HashSet<TransactionId>
deposits: HashMap<TransactionId, DepositRecord>
```

Responsibilities:

- `accounts` stores current account state.
- `seen_transaction_ids` prevents duplicate deposits and withdrawals.
- `deposits` stores only information needed by future dispute lifecycle events.

A deposit identifier is present in both the set and the deposit map. This is intentional: the set is an idempotency index, while the map stores dispute state.

Withdrawal details are not retained after processing.

## Chosen architecture

The implementation uses a lightweight ports-and-adapters structure in a single Cargo crate:

```text
CSV file
   ↓
CSV inbound adapter
   ↓
ProcessTransaction inbound port
   ↓
PaymentEngine application service
   ↓
PaymentRepository outbound port
   ↓
InMemoryPaymentRepository outbound adapter
```

The file reader is an inbound adapter. It is not part of the domain and is not the inbound port.

## Identity model

- A deposit or withdrawal introduces a primary financial transaction.
- Its `tx` is globally unique.
- A dispute, resolve, or chargeback references an existing deposit using the same `tx`.
- Lifecycle rows do not introduce new transaction identities.
- `(type, tx)` is not the global transaction identity.
- Primary transaction duplicate protection uses `seen_transaction_ids`.
- Lifecycle repetition is validated through deposit state.
- Exact event-level idempotency is unavailable because the input has no independent event identifier.

## How to use these specifications

Each business rule should be represented by at least one unit or acceptance test.

When implementation and specification disagree, either correct the implementation or update the specification and document why the behavior changed.

Do not copy external assessment documents into the repository. Keep these files original, concise, and implementation-oriented.
