# Architecture and Ports

## Style

Use a lightweight ports-and-adapters architecture in one Cargo crate.

```text
External input
    ↓
Inbound adapter
    ↓
Inbound port
    ↓
Application service
    ↓
Outbound port
    ↓
Outbound adapter
```

## Recommended layout

```text
src/
├── main.rs
├── domain/
│   ├── account.rs
│   ├── transaction.rs
│   ├── deposit.rs
│   └── outcome.rs
├── application/
│   ├── payment_engine.rs
│   ├── changes.rs
│   └── ports/
│       ├── inbound.rs
│       └── outbound.rs
└── adapters/
    ├── inbound/csv.rs
    └── outbound/memory_repository.rs
```

## Domain

Contains pure business concepts and must not depend on CSV, files, command-line arguments, database clients, `HashMap`, `HashSet`, or `anyhow`.

## Inbound port

```rust
pub trait ProcessTransaction {
    type Error;

    fn process(
        &mut self,
        transaction: Transaction,
    ) -> Result<ApplyOutcome, Self::Error>;
}
```

An optional `ListAccounts` query port may expose final account state.

## CSV inbound adapter

The file reader is an inbound adapter, not a port.

It owns CSV deserialization, external validation, row-to-domain conversion, streaming, and invocation of `ProcessTransaction`.

A separate `TransactionSource` port is intentionally omitted because the application does not currently need to pull from multiple source types.

### Internal seam

The adapter is split into two functions:

```text
fn parse_rows(reader) -> impl Iterator<Item = Result<Transaction, AdapterError>>
fn drive<I, P>(source: I, port: &mut P) -> Result<(), AdapterError>
    where I: Iterator<Item = Result<Transaction, AdapterError>>,
          P: ProcessTransaction
```

`drive` is the reusable piece. It is not a port; it is an internal helper that lets the same driving logic accept a channel-backed or in-memory iterator without introducing a public transaction-source abstraction.

## Outbound repository port

```rust
pub trait PaymentRepository {
    type Error;

    fn transaction_seen(
        &self,
        tx: TransactionId,
    ) -> Result<bool, Self::Error>;

    fn account(
        &self,
        client: ClientId,
    ) -> Result<Option<Account>, Self::Error>;

    fn deposit(
        &self,
        tx: TransactionId,
    ) -> Result<Option<DepositRecord>, Self::Error>;

    fn commit(
        &mut self,
        changes: LedgerChanges,
    ) -> Result<(), Self::Error>;

    fn accounts(&self) -> Result<Vec<Account>, Self::Error>;
}
```

The application validates business rules and submits one atomic `LedgerChanges` value.

## In-memory outbound adapter

```rust
pub struct InMemoryPaymentRepository {
    accounts: HashMap<ClientId, Account>,
    seen_transaction_ids: HashSet<TransactionId>,
    deposits: HashMap<TransactionId, DepositRecord>,
}
```

Collection details remain inside this adapter.

## Future database adapter

A database implementation may use accounts, seen-transactions, and deposits tables. Its `commit` implementation must persist related changes in one database transaction. A unique constraint on transaction ID provides database-level duplicate protection.

## Composition root

`main.rs` wires:

```text
File
  -> CSV inbound adapter
  -> PaymentEngine<InMemoryPaymentRepository>
  -> CSV output adapter
```

`anyhow` is allowed only here for CLI and I/O context.

## Dependency rules

Allowed:

```text
adapters -> application ports
application -> domain
application -> outbound port
main -> concrete components
```

Forbidden:

```text
domain -> adapters
application -> CSV or filesystem
application -> concrete in-memory collections
```
