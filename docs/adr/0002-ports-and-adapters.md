# ADR 0002: Lightweight Ports and Adapters

## Status

Accepted

## Decision

Use a lightweight ports-and-adapters architecture in one Cargo crate.

```text
Inbound port:   ProcessTransaction
Outbound port:  PaymentRepository
Inbound adapter: CSV reader
Outbound adapter: In-memory repository
```

The CSV file reader is an adapter, not a port.

The repository port exposes an atomic commit operation so a future database adapter can persist a complete ledger change in one transaction.

## Consequences

Positive:

- business rules are independent from CSV and storage;
- application tests do not require files;
- a database adapter can be added without changing processing rules;
- atomicity is explicit.

Negative:

- more modules and interfaces than a direct procedural solution;
- change-set design requires care.

## Guardrails

- Keep one Cargo crate.
- Keep the inbound port narrow.
- Do not create a generic transaction-source port for the current scope.
- Do not expose collections through the repository interface.
- Keep `anyhow` at the binary boundary.
