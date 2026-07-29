# Input and Output Contract

## Command-line interface

```text
cargo run -- transactions.csv
```

The binary accepts one input CSV path and writes final account rows to standard output.

## Input columns

```text
type,client,tx,amount
```

Supported types:

```text
deposit
withdrawal
dispute
resolve
chargeback
```

## Field rules

- `client` is `u16`.
- `tx` is `u32`.
- amounts support up to four fractional places.
- surrounding whitespace is accepted.
- amount is required for deposits and withdrawals.
- input order is chronological.

## Identity contract

- Deposit and withdrawal rows introduce primary transactions.
- Their `tx` values are globally unique.
- The first observed primary transaction reserves `tx`, even when rejected.
- Lifecycle rows reference a deposit with the same `tx`.
- Lifecycle rows are not inserted into `seen_transaction_ids`.
- `(type, tx)` is not the identity key.

## Input adapter contract

The CSV reader is an inbound adapter. It must:

1. stream one row at a time;
2. deserialize and validate external fields;
3. convert rows into domain `Transaction` values;
4. invoke the `ProcessTransaction` inbound port;
5. continue after domain rejections;
6. stop after parsing, application, or repository errors.

The application layer must not depend on file paths, `csv::Reader`, or `std::fs::File`.

## Money representation

Use exact decimal arithmetic. Floating-point balances are not permitted.

## Output columns

```text
client,available,held,total,locked
```

Rules:

- one row per client;
- `total = available + held`;
- precision up to four decimal places;
- row order is not significant;
- no diagnostics on `stdout`.
