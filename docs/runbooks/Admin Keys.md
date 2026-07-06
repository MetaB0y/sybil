# Admin Keys

Status: current as of 2026-07-06 (SYB-96/97 landing). Single-operator phase.

## Who holds what

One admin key, held by Valery. It is the `admin` on both `SybilVault` and
`SybilSettlement` (no multisig at this stage — revisit before any real-funds
deployment). There are no other roles: the previous PAUSER/GUARDIAN/VERIFIER_ADMIN
role set was collapsed into single-admin + timelock when the contracts gained
the in-contract timelock (`contracts/src/access/SybilAccessControl.sol`).

## Immediate powers (no timelock)

- `pause()` / `unpause()` on either contract. Pause is deliberately immediate —
  it is the incident-response brake. Paused vault: no deposits, no withdrawal
  requests, no finalization. Paused settlement: no new root acceptance.
- `cancelWithdrawal(nullifier)` on a **queued** withdrawal during its delay
  window (fraud response). Funds stay in the vault; the user can re-request.
  After `executableAt` the cancel reverts (`WithdrawalCancelWindowElapsed`).

## Timelocked powers (propose → wait → execute)

Default delay 48h (`initialAdminActionDelay`, constructor-configurable per
deployment; devnet uses shorter values in tests). Each operation is keyed by a
domain constant; proposals can be canceled by the admin before execution.

- `OP_SET_VERIFIER` (settlement): swap the verifier adapter — i.e. repin guest
  commitments outside a redeploy.
- `OP_SET_WITHDRAWAL_DELAY` (vault): change the withdrawal queue delay
  (default 24h).
- `OP_SET_ESCAPE_TIMEOUT` (vault): change the escape-mode staleness timeout.
- `OP_ADMIN_TRANSFER`, `OP_ADMIN_ACTION_DELAY` (both): rotate the admin key or
  change the timelock delay itself.

## Withdrawal delay — the user-facing tradeoff

Withdrawals are queued on-chain for 24h (default) before `finalizeWithdrawal`
becomes callable by anyone. The delay exists so a rogue-sequencer or bad-proof
incident can be caught and paused/canceled before funds leave the vault. Users
see `l1_executable_at` on the withdrawal status API.

## Key rotation / loss

Rotation goes through `OP_ADMIN_TRANSFER` (timelocked). Loss of the single
admin key means: no pause, no cancels, no parameter changes — the exchange
keeps running and user withdrawals still finalize permissionlessly, but there
is no incident brake. Store the key accordingly; a hardware key or at minimum
an encrypted offline backup is expected before opening deposits beyond dev
funds.
