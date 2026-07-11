# Secrets and API keys

> **Rule:** never commit a live secret. Runtime secrets belong in host-side env
> files or an operator's local secret store; documentation records purpose and
> custody, never values.

## Secret classes

| Secret | Purpose | Runtime location |
|---|---|---|
| Service bearer token | Authenticates first-party service/operator routes | `/opt/sybil/.env` |
| Grafana administrator password | Protects the Grafana account | `/opt/sybil/.env` |
| Caddy operations credentials | Protects operator-facing HTTP surfaces | `/opt/sybil/.env` |
| WebAuthn RP/origin configuration | Binds browser assertions to the deployed origin | `/opt/sybil/.env` |
| OpenRouter provider key | Allows arena agents to call configured LLMs | `/opt/sybil/arena.env` |
| Contract/feed signing keys | Controls privileged on-chain or resolution actions | Operator custody; see [Admin keys](runbooks/Admin%20Keys.md) |

The authoritative required-variable list and deployment checks live in
[`DEPLOY.md`](https://github.com/MetaB0y/sybil/blob/main/DEPLOY.md). Compose and the `justfile` should fail before a
production deploy if required entries are absent.

## Rotation checklist

1. Create the replacement in the provider or custody system.
2. Update the host-side env/secret store without printing the value to logs,
   shell history, process arguments, or review output.
3. Restart only the consumers of that secret and run the relevant smoke checks.
4. Revoke the old credential and verify that it no longer works.
5. If a value ever entered version-control history, treat it as compromised;
   deletion from the current tree is not revocation.

User-created read API keys are a different class: they are read-only account
credentials created and revoked through the account settings/API. They cannot
authorize trades, withdrawals, or signing-key changes; those require a
registered P256/WebAuthn signing key.
