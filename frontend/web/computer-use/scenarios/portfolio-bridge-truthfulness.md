---
id: portfolio-bridge-truthfulness
priority: p0
mode: read-only
personas: connected account holder
routes: /portfolio
fixtures: disposable account with known bridge deposit and withdrawal lifecycle
environments: desktop,secure-context,passkey-authenticator
---

# Reconcile portfolio and bridge status without false promises

## Intent

Confirm that an account holder can distinguish exchange balance, L1 deposit
routing, normal-withdrawal lifecycle, and currently unavailable product actions
without confusing keys, mistaking missing history for zero, or believing mock
proof plumbing protects real funds.

## Preconditions

- Provision a disposable account whose expected exchange balance, exact L1
  deposit routing key, and at least one known withdrawal status are recorded by
  the fixture provider.
- The fixture may be queued, finalizable, finalized, cancelled, or refunded, but
  its expected status and relevant timestamps must be known before the run.
- Connect only with the fixture account's passkey in a fresh browser profile.

## Steps

1. Open Portfolio and wait for account, positions, orders, history, bridge key,
   and withdrawal reads to reach explicit settled states.
2. Compare the visible balance and activity with the fixture's known deposit and
   withdrawal amounts without assuming that L1 token units equal display nanos.
3. Inspect the configured chain, vault, collateral token, L1 routing-key label,
   selection guidance, recovery explanation, and quarantine limitation language.
4. Inspect the normal-withdrawal region, the fixture row's status, timestamps,
   countdown or finalizable state, and any explanation of unavailable actions.
5. Reload Portfolio once and confirm the same authoritative bridge lifecycle is
   still shown rather than a stale creation-time snapshot.

## Observable assertions

- Loading, unavailable, stale, successful-empty, and populated private reads are
  visibly different; a failed bridge read never becomes “no withdrawals.”
- The 32-byte deposit routing key is clearly named as an account routing value
  and is never described as a passkey, signing public key, wallet address, or
  secret.
- The configured chain, vault, and token are all visible before the routing key;
  if any are unavailable, the panel says not to deposit rather than showing a
  partially actionable instruction.
- Deposit guidance does not claim that a specific deposit is quarantined when
  only aggregate quarantine data exists, and it states current recovery or
  refund limitations honestly.
- Withdrawal status comes from current indexed L1 lifecycle data. A countdown
  appears only with a known executable time and never implies automatic
  finalization.
- The web app offers no normal-withdrawal creation action while its signed
  proof-backed product flow is disabled, and it does not describe the unsafe
  mock verifier as real-funds security.
- Reload preserves the fixture's exact status and does not double-count the
  deposit, withdrawal debit, or final payout in portfolio totals.

## Evidence

- Capture the settled balance, deposit panel, complete withdrawal row, disabled
  or absent creation affordance, and the same row after reload.
- Record expected versus visible amounts and lifecycle status, including the
  unit labels used by the product.
- Redact the routing key in shared screenshots except for enough prefix and
  suffix to prove it matches the bound fixture.

## Cleanup

- Disconnect the fixture account locally and close any copied-value notice.
- No server-side product state was changed.

## Stop conditions

- Stop as blocked if the fixture provider cannot establish expected balances,
  routing key, and lifecycle status independently of the UI.
- Stop as failed if the product offers an unsafe release action, conflates the
  routing and signing keys, or renders a read failure as successful emptiness.
- Stop without copying or exposing any secret if the browser or evidence tool
  cannot keep private account data out of shared artifacts.
