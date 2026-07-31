# Rejections and Errors

The system distinguishes expected domain rejections from fatal application or adapter errors.

## Domain outcome

```rust
pub enum ApplyOutcome {
    Applied,
    Rejected(RejectionReason),
}
```

Suggested rejection reasons:

```rust
pub enum RejectionReason {
    AccountLocked { client: u16 },
    InvalidAmount,
    DuplicateTransaction { tx: u32 },
    InsufficientFunds { client: u16 },
    DepositNotFound { tx: u32 },
    ClientMismatch {
        tx: u32,
        expected_client: u16,
        actual_client: u16,
    },
    DepositAlreadyDisputed { tx: u32 },
    DepositNotDisputed { tx: u32 },
    DepositAlreadyChargedBack { tx: u32 },
}
```

Processing continues after a domain rejection.

## Application and repository errors

```rust
pub enum EngineError<E> {
    Repository(E),
    InvariantViolation(&'static str),
}
```

These failures stop processing because correct state can no longer be guaranteed.

## CSV adapter errors

The inbound CSV adapter owns format failures:

- malformed CSV;
- invalid transaction type;
- invalid identifiers;
- invalid decimal;
- missing amount for deposit or withdrawal.

CSV parsing failures are not domain rejections.

## CLI and I/O errors

The composition root owns missing arguments, file-open failures, output failures, and flush failures.

`anyhow` may be used only at this outer boundary.

## Error behavior

```text
Applied domain event  -> continue
Rejected domain event -> continue
CSV parsing error     -> stop
Repository error      -> stop
Output error          -> stop
```

Required account CSV is written only to `stdout`; optional diagnostics use `stderr`.
