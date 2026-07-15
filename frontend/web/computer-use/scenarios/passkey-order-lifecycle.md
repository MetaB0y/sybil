---
id: passkey-order-lifecycle
priority: p0
mode: disposable-account
personas: new trader
routes: /,/m/:market_id,/portfolio
fixtures: public demo-account capacity,active market suitable for a resting order
environments: desktop,secure-context,passkey-authenticator
---

# Create a passkey account and control a resting order

## Intent

Confirm the core devnet promise: a new trader can create a passkey-backed demo
account, submit one deliberately resting signed order, observe its reservation
after reload, cancel it, and reconnect without exposing signing material or
duplicating a mutation.

## Preconditions

- Use a fresh browser profile and a new passkey authenticator created only for
  this run.
- Confirm public demo-account capacity remains and bind an active market where a
  small limit order can rest without crossing current interest.
- Record the starting revision, origin, visible market name, and a unique run
  label; do not reuse an account or credential from another run.

## Steps

1. Open Connect, choose the default passkey account path, complete one browser
   passkey ceremony, and wait for the connected account summary.
2. Open the bound market, choose advanced order entry, select a small quantity
   and a limit visibly away from execution, and review the side, outcome, limit,
   quantity, reservation, and time in force before confirming once.
3. Wait for an accepted result, visit Portfolio, and locate the order under open
   orders together with the resulting available-balance reservation.
4. Reload Portfolio and confirm the same account and order return from server
   state rather than appearing only in the previous page session.
5. Cancel that order through the signed confirmation flow, wait for the open
   order to disappear, and verify that available balance reconciles.
6. Disconnect locally, choose the existing-account passkey path, reconnect with
   the run's passkey, and confirm Portfolio still identifies the same account
   with no open order from this scenario.

## Observable assertions

- Passkey is the default secure account path when supported; no private key,
  credential identifier, or reusable secret is displayed or requested.
- A busy submission disables or otherwise guards the final action so one user
  confirmation cannot create duplicate orders.
- The accepted message describes what was accepted without promising a fill;
  the resting order shows the exact side, limit, remaining quantity, and time in
  force that the trader reviewed.
- Available balance reflects the reservation while the order rests and releases
  it after cancellation without changing the account's total cash dishonestly.
- Reload and reconnect recover authoritative state; neither a loading failure
  nor an unavailable private read is rendered as zero balance or no orders.
- Cancellation has a visible success or actionable failure state and cannot
  silently remove only the local row.

## Evidence

- Capture the account-created state, final order review, accepted result, open
  order before and after reload, cancellation result, and reconnected Portfolio.
- Record only the opaque account and order numbers needed to reconcile the run;
  redact credential material, challenges, signatures, and authorization data.
- Record the visible balance and reservation before submission, while resting,
  and after cancellation, plus unexpected console errors or failed requests.

## Cleanup

- Cancel every open order created by this run and verify each is absent after a
  fresh Portfolio read.
- Disconnect the disposable account locally; the empty test account and its
  passkey registration may remain on the devnet.
- Never cancel or alter any order that was not created by this run.

## Stop conditions

- Stop before mutation if demo-account capacity, a fresh authenticator, or a
  market suitable for a non-crossing order is unavailable.
- Stop immediately if the UI selects a pre-existing account, requests raw
  signing material, changes the reviewed values, or requires repeated final
  confirmation because the first result is merely slow.
- Stop and perform cleanup if the market resolves, pauses, or changes so the
  proposed limit may execute before submission.
