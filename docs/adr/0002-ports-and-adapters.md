# ADR 0002: Lightweight Ports and Adapters

## Status

Accepted

## Decision

Use a lightweight ports-and-adapters architecture in one Cargo crate.

```text
Inbound port:   ProcessTransaction, ListAccounts (both public)
Outbound port:  LedgerRepository (public trait, effectively sealed —
                LedgerChanges has pub(crate) fields so no out-of-crate
                implementer can inspect the change-set it would receive)
                type Error: std::error::Error + Send + Sync + 'static
Inbound adapter:  CSV reader
Outbound adapter: In-memory repository
```

The CSV file reader is an adapter, not a port.

The trait itself is `pub` because it appears as a public generic
bound on `PaymentEngine`, but it takes a `LedgerChanges` value whose
fields are `pub(crate)`: an out-of-crate implementer would receive a
change-set it cannot inspect, so any additional repository adapter
must live inside this crate. The port still exposes an
atomic commit operation so a future in-crate database adapter can
persist a complete ledger change in one transaction — see ADR 0003 for
the atomicity caveat.

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
