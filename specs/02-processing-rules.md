# Processing Rules

Events are processed sequentially in input order.

Each application call returns:

```text
Applied
Rejected(reason)
```

A domain rejection does not stop processing.

## Primary transaction pre-processing

Deposit and withdrawal rows follow this sequence:

1. Check whether `tx` exists in `seen_transaction_ids`.
2. If present, reject as `DuplicateTransaction`.
3. Otherwise, insert `tx` into `seen_transaction_ids`.
4. Validate the business operation.
5. Apply balance changes only when validation succeeds.
6. Retain a `DepositRecord` only for an applied deposit.

## Deposit

Preconditions:

- `tx` has not been seen.
- account is not locked.
- amount is positive.

Applied effects:

```text
available += amount
held       unchanged
total     += amount
```

Store an applied `DepositRecord`.

Rejected effects:

- balances remain unchanged;
- no deposit record is created;
- `tx` remains reserved.

## Withdrawal

Preconditions:

- `tx` has not been seen;
- account is not locked;
- amount is positive;
- `available >= amount`.

Applied effects:

```text
available -= amount
held       unchanged
total     -= amount
```

No withdrawal record is retained.

Rejected effects:

- balances remain unchanged;
- no withdrawal record is retained;
- `tx` remains reserved.

## Dispute

Preconditions:

- referenced deposit exists;
- deposit belongs to the supplied client;
- account is not locked;
- deposit status is `Applied`.

Effects:

```text
available -= deposit.amount
held      += deposit.amount
total      unchanged
Applied   -> Disputed
```

## Resolve

Preconditions:

- referenced deposit exists;
- deposit belongs to the supplied client;
- account is not locked;
- deposit status is `Disputed`.

Effects:

```text
available += deposit.amount
held      -= deposit.amount
total      unchanged
Disputed  -> Applied
```

## Chargeback

Preconditions:

- referenced deposit exists;
- deposit belongs to the supplied client;
- deposit status is `Disputed`.

Effects:

```text
available  unchanged
held      -= deposit.amount
total     -= deposit.amount
locked     = true
Disputed   -> ChargedBack
```

## Atomicity

A primary operation atomically performs:

```text
reserve tx
+ optional account mutation
+ optional deposit insertion
```

A lifecycle operation atomically performs:

```text
account mutation
+ deposit status transition
```

The in-memory adapter performs these changes synchronously. A future database adapter must use a database transaction or equivalent conditional atomic write.
