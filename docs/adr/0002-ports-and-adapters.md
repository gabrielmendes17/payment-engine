# ADR 0002: Lightweight Ports and Adapters

## Status

Accepted

## Decision

Use a lightweight ports-and-adapters architecture in one Cargo crate.

```text
Inbound ports:  ProcessTransaction, ListAccounts (public traits;
                associated Error bounded on std::error::Error + Send +
                Sync + 'static)
Outbound port:  LedgerRepository (public trait; associated Error same
                bound). Committed change-sets are destructured via
                LedgerChanges::into_parts() so any adapter can persist
                them however its storage requires.
Inbound adapter:  CSV reader
Outbound adapter: In-memory repository
```

The CSV file reader is an adapter, not a port.

The outbound port exposes an atomic commit operation so a database
adapter can persist a complete ledger change in one transaction — see
ADR 0003 for the atomicity caveat.

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
