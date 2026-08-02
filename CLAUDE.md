# Payment engine — Claude notes

Durable context for anyone (human or Claude) working on this repo. Kept short
on purpose. If a rule below reads as obvious, delete it.

## Commands

- Build: `cargo build`
- Format check: `cargo fmt --check`
- Lint (strict, matches CI): `cargo clippy --all-targets --all-features -- -D warnings`
- Tests: `cargo test`
- Run against a fixture: `cargo run -- tests/fixtures/spec_sample.csv`

Run all four before every commit.

## Architecture rules

- Hexagonal layout: `domain/` → `application/` → `adapters/`. The domain layer
  must not import from `application` or `adapters`. The application layer must
  not import from `adapters`.
- Use cases own `LedgerChanges` construction and the single
  `repository.commit(...)` call. `PaymentEngine` only dispatches.
- No blanket `From<AccountError> for RejectionReason` or
  `From<DisputeError> for RejectionReason`. Route every domain error through
  `helpers::classify_account_error` / `helpers::classify_dispute_error` so
  `AccountError::ArithmeticOverflow` cannot silently downgrade to a business
  rejection.
- `From<DepositError> for RejectionReason` is kept because every
  `DepositError` variant is a true business rejection. If a fatal variant is
  ever added to `DepositError`, delete that impl and add a
  `classify_deposit_error` at the same time.
- `anyhow` is confined to `src/main.rs` for operational context on stderr.
  The domain and application layers use `thiserror` only.
- `Account` balance methods consume `self` and return the updated entity, so
  a failed operation cannot leak partial mutation.

## Business rules to preserve

- `Account::hold` may take `available` negative when disputed funds have
  already been spent. Do not add an insufficient-funds check there. This is
  covered by `hold_may_make_available_negative_scenario_12`.
- A rejected primary transaction (deposit / withdrawal) still reserves its
  `tx` so the number cannot be reused within the same run. Lifecycle events
  (dispute / resolve / chargeback) do not reserve `tx`.
- The chargeback lock is a per-account kill-switch, not a per-deposit flag.
  After a chargeback, every subsequent operation on that account is rejected —
  including a chargeback of a different disputed deposit on the same account.
- Lifecycle events only apply to deposits. A dispute / resolve / chargeback
  referencing a withdrawal `tx` (or an unknown `tx`) is `DepositNotFound`,
  not an error.
- Ownership guard: dispute / resolve / chargeback require the referenced
  deposit to belong to the transaction's client. Mismatches are
  `ClientMismatch`.

## Error taxonomy

Terminates CSV processing (returns `Err`):
- `EngineError::Repository(_)`
- `EngineError::ArithmeticOverflow { client }`
- `CsvInputError::Csv`, `UnknownType`, `MissingAmount`

Business outcome (returns `Ok(ApplyOutcome::Rejected(_))`):
- Every current `RejectionReason` variant.

Balance arithmetic uses `checked_add` / `checked_sub`. `Account::credit`
additionally validates the resulting `available + held` via
`ensure_total_representable` because that is the only operation whose total
can grow. The other four methods either preserve the total (`hold`,
`release`) or strictly shrink it (`debit`, `mark_charged_back`), so the guard
is unnecessary and was deliberately removed from them.

## Style

- No `expect` / `unwrap` / `panic!` on paths reachable from CSV input.
- Comments only for surprising business rules or non-obvious code. Do not
  restate what the code already says. Test names should be descriptive
  enough to replace a doc-comment.
- No emojis in code, tests, or docs unless the user asks for them.
- Reject changes that add backwards-compatibility shims, feature flags for
  scenarios that can't happen, or defensive code beyond system boundaries.

## Repository hygiene

- Do not commit the challenge PDF or any derivative.
- Do not add company, brand, or product names to code or docs. Refer to the
  project generically ("Payment Engine", "the challenge").
- Ad-hoc fixtures live outside `tests/fixtures/` (e.g. `sample.csv`,
  `sample_overflow.csv`). Only commit them under `tests/fixtures/` if a test
  uses them.

## When something looks broken in the IDE

If your IDE reports errors like "Trait `Eq` is private" or "Method `clone`
not found on Account" while `cargo build` / `cargo test` succeed, the IDE
lost its Rust project index. In JetBrains: **File → Invalidate Caches /
Restart**. In VS Code with rust-analyzer: **Command Palette →
`rust-analyzer: Restart server`**. Trust `cargo` over the IDE.
