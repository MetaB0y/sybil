# Passkey recovery

> **Executive summary:** Sybil cannot reset a passkey or recover one from the
> server. A synced passkey or a second registered signing key is the recovery
> path. Add and test that backup while an existing key still works.

Sybil passkey accounts use P256 keys with `auth_scheme = webauthn`. The account
commits its active key set, and every additional-key operation must be signed by
an existing key against the current key/event digests. The unsigned bootstrap
endpoint can register only an account's first key and cannot recover an account
that already has one.

## Recommended setup

1. Create the account with a passkey from a synced passkey provider where
   appropriate.
2. While signed in with that key, open **Settings → Signing keys / agent keys**
   and choose **Add backup passkey**. Use the browser or authenticator picker to
   create the additional credential on the intended device or provider.
3. Sybil fetches the current key-operation binding and has the existing passkey
   authorize the state-bound registration. The server never receives either
   passkey's private key and cannot reset one.
4. Disconnect, choose **Sign in with passkey**, and select the backup. Confirm
   that it reconnects to the same account and sign a harmless action before
   relying on it.

Disconnecting or clearing local browser data does not delete a synced or
authenticator-held passkey. It also does not reset server state: discoverable
sign-in recovers the account id from the selected passkey. Do not ask an
operator to bypass the signed registration path or reset the account.

## Lost device

- If a synced or backup passkey remains available, sign in with it, add a
  replacement, test the replacement, then revoke the lost key.
- If no registered signing key remains usable, the account cannot authorize new
  orders, cancellations, key changes, or withdrawals. There is no seed phrase,
  email reset, or server-side override by design.
- Read-only API keys do not help: they deliberately have no mutation authority.

## Release test matrix

Before enabling a production origin, test create, discover/sign-in, order,
cancel, backup registration, and revocation on:

- iOS Safari and Android Chrome;
- macOS Safari and Chrome;
- Windows Edge/Chrome with Windows Hello;
- at least one cross-device or synced-passkey flow.

Each device must use strictly increasing nonces, and WebAuthn assertions must
bind the configured RP ID and origin. See [[P256 Authentication]] and
[ADR-0014](adr/0014-webauthn-first-auth.md) for the protocol model.
