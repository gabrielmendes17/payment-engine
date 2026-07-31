# Acceptance Scenarios

## Domain and application

1. An applied deposit credits available and total, reserves its `tx`, and stores a deposit record.
2. An applied withdrawal debits available and total, reserves its `tx`, and stores no withdrawal record.
3. An insufficient withdrawal is rejected, reserves its `tx`, and changes no balance.
4. Reusing a seen `tx` for any deposit or withdrawal is rejected as `DuplicateTransaction`.
5. A dispute moves the deposit amount from available to held and changes status to `Disputed`.
6. A resolve moves held funds back to available and returns status to `Applied`.
7. A chargeback removes held funds, changes status to `ChargedBack`, and locks the account.
8. Resolve or chargeback before dispute is rejected without state change.
9. An unknown deposit reference is rejected.
10. A cross-client reference is rejected.
11. A duplicate dispute is rejected by lifecycle state.
12. A dispute may make available negative after prior spending.
13. New primary activity on a locked account is rejected and its `tx` remains reserved.

## Repository adapter

14. An applied deposit ID exists in both `seen_transaction_ids` and `deposits`.
15. A withdrawal ID exists only in `seen_transaction_ids`.
16. Repository commit applies all event changes atomically.
17. A failed commit exposes no partial account or deposit mutation.

## Inbound CSV adapter

18. A valid CSV row converts into the correct domain transaction.
19. Missing deposit or withdrawal amount fails before invoking the application.
20. A domain rejection does not stop the CSV stream.
21. A malformed CSV row stops processing with an adapter error.

## End-to-end

Input:

```csv
type,client,tx,amount
deposit,1,1,100.0000
withdrawal,1,2,30.0000
dispute,1,1,
chargeback,1,1,
```

Expected logical output:

```csv
client,available,held,total,locked
1,-30.0000,0.0000,-30.0000,true
```
