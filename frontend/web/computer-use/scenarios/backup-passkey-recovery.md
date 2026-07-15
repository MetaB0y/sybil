---
id: backup-passkey-recovery
priority: p1
mode: disposable-account
personas: account holder preparing recovery
routes: /settings,/portfolio
fixtures: disposable passkey account
environments: desktop,secure-context,two independent passkey authenticators
---

# Add a backup passkey and recover the same account

## Intent

Confirm that a user can understand the active signing-key set, authorize a
second passkey from the existing account, recover with that independent device,
and manage a show-once read key without losing the last signing path.

## Preconditions

- Use a disposable account with one working primary passkey and no other keys.
- Provide a genuinely independent second authenticator or virtual device for
  the backup credential.
- Record the opaque account number and visible primary-key label; never export
  credential secrets into the run record.

## Steps

1. Connect with the primary passkey, open Settings, and identify profile,
   signing-key, backup, and read-key regions plus their security explanations.
2. Start Add backup passkey, create the credential on the second authenticator,
   authorize the account change with the primary passkey, and wait for the new
   labeled key to appear exactly once.
3. Create one uniquely labeled read API key, inspect its show-once warning, try
   the visible copy action, then close the dialog and verify the secret cannot
   be reopened from the key list.
4. Disconnect, make the primary authenticator unavailable, and use the existing
   account passkey path with only the backup authenticator active.
5. Confirm the same account opens, visit Portfolio, then return to Settings and
   revoke the read API key using the backup passkey.
6. Attempt to revoke whichever signing key would be the last remaining active
   key only far enough to inspect the prevention or confirmation language; do
   not approve a change that could strand the account.

## Observable assertions

- The product distinguishes signing passkeys from read API keys and explains
  which actions each can authorize.
- Backup registration requires current-account authorization and cannot create
  duplicate visible keys after slow or repeated rendering.
- The read API key secret is explicitly show-once, is not placed in ordinary
  page text after close, and never claims clipboard success when copying fails.
- Backup-only sign-in recovers the exact same account rather than onboarding a
  new one or falling back to locally cached portfolio data.
- Revocation is state-bound, has a visible result, and never permits removal of
  the final active signing key.
- Loading or failed Settings reads never render an empty key list that invites
  unsafe duplicate registration.

## Evidence

- Capture the initial key inventory, backup-added result, redacted show-once
  dialog, backup-only reconnection, read-key revocation, and last-key prevention.
- Record visible labels and opaque account/key references only; redact every
  secret, credential identifier, challenge, signature, and authorization value.
- Record which authenticator was active for each ceremony and any unexpected
  console error or failed request.

## Cleanup

- Revoke the read API key created by this run and verify it is no longer active.
- Leave both signing passkeys registered so the disposable account is not
  intentionally stranded, then disconnect locally.
- Do not modify any key whose label or opaque identity predates this run.

## Stop conditions

- Stop before mutation if either authenticator cannot be isolated or the account
  has an unexpected pre-existing key.
- Stop immediately if a secret appears in logs or screenshots, the backup path
  resolves to another account, or the product proposes removing the last key.
- Stop and preserve both signing paths if authorization state changes between
  review and confirmation.
