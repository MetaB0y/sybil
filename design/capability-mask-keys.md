---
tags: [design, keys, auth, consensus]
layer: core
status: exploratory
date: 2026-07-07
---

# Capability-masked keys — scoped delegated authority

Expands brainstorm idea #9 ([[possibility-space-2026-07]]) and the deferred
ratification decision **D1** ([`keys-and-escape-ratification.md`](keys-and-escape-ratification.md)).
The small feature that makes the **agent arena** (idea #8) and managed accounts
*safe*: a key that can trade but not withdraw.

## The problem

Today **every registered key can sign every mutation** — order, cancel,
withdrawal, key-registration, escape (`crypto.rs`: `scope` is descriptive
metadata only, [[P256 Authentication]]). So handing a key to a trading bot, a
third-party agent, or a session hands over **full custody**. For an exchange
whose flagship growth surface is an arena of bots (idea #8), that's a
non-starter — nobody delegates a withdraw-capable key to an LLM.

## The design

A **`capability_mask`** per registered key: a small bitfield of what that key is
allowed to authorize.

```
TRADE        = 1 << 0   // place / cancel orders
WITHDRAW     = 1 << 1   // initiate a withdrawal
ESCAPE       = 1 << 2   // sign an escape claim
MANAGE_KEYS  = 1 << 3   // register / revoke keys
// reserved bits for future capabilities
```

- **Delegated/agent keys** get `TRADE` only. A compromised bot key can lose you
  *trading P&L*, never your *collateral*.
- **Owner keys** get the full mask (all bits) — the default, for backward
  compatibility.
- **Enforcement** is at authorization time: the same point that checks the
  signature checks the mask. Today that's the API/actor boundary; once in-guest
  intent verification lands (the ZK-8 closure path, [[Threat Model]]), the mask
  is checked *in the guest* too — at which point delegation is trust-minimized.

### It must be in the commitment

The mask has to be **consensus-visible**, or a malicious operator could ignore
it. So it's digested into `keys_digest` alongside the key: extend the
`key_record` in [`account-keys-digest.md`](account-keys-digest.md) from
`auth_scheme:u8 || pubkey_sec1[33]` to
`auth_scheme:u8 || capability_mask:u32 || pubkey_sec1[33]`. Then escape and
operator-replacement recover the mask with the key, and the guest can enforce it.

## The forward-compatibility recommendation (refines D1)

D1 recommended keeping scope cosmetic in v1 and adding a real `capability_mask`
*later*. But `keys_digest` is a **consensus encoding** — adding a field to it
later is a second schema move and a second fresh-genesis redeploy
([ADR-0009](../docs/adr/0009-fresh-genesis-for-consensus-changes.md)). That's
expensive to pay twice.

**Recommendation: reserve the `capability_mask` field in the v4 `keys_digest`
encoding now**, defaulting every key to the full mask (all bits set = today's
behavior). This is nearly free — one `u32` in the key record — and it means
*activating* scoped delegation later is a pure **application-layer** change (start
issuing restricted masks; start enforcing them) with **no further consensus
schema move**. Semantics stay deferred (D1's intent); only the *byte slot* is
claimed now, while we're already moving the schema for SYB-225.

> The general principle: when you're already paying for a consensus schema change,
> reserve the fields you can foresee needing. A byte slot is cheap; a second
> fresh-genesis is not.

## Safety constraints

- **Escape must never be fully delegated away.** Every account must retain at
  least one key with `ESCAPE` (and `WITHDRAW`) authority, or the owner could lock
  themselves out of the exit ([ADR-0005](../docs/adr/0005-escape-via-operator-replacement.md)).
  Enforce "≥1 full-authority key" as an invariant on key mutations.
- **A mask is only as strong as its weakest enforcement point.** Until it's
  checked in-guest, a restricted key is only *operationally* restricted (the
  honest operator honors it); the mask becomes *trust-minimized* only when the
  guest enforces it. Document this precisely — don't over-claim.
- **Masks are set at registration and are immutable per key** (change = revoke +
  re-register). Simpler to reason about than mutable per-key policy; the key *is*
  the capability.

## What it unlocks

- **The agent arena (idea #8):** users delegate `TRADE`-only keys to bots — the
  precondition for a safe permissionless bot ecosystem.
- **Managed / institutional accounts:** a manager trades; only the owner
  withdraws.
- **Session keys:** short-lived `TRADE` keys for a web session, revocable without
  touching the owner key.

Small, bounded, and consensus-additive — but only if the byte slot is reserved in
the same schema move as `keys_digest`. That timing is the one decision to make
now; everything else can wait.
