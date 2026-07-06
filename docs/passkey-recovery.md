# Passkey Recovery

Sybil passkey accounts are P256 account keys registered with `auth_scheme = webauthn`.
The server does not hold recovery material. Recovery means adding another
passkey-backed P256 key to the same Sybil account while the user can still sign
with an existing key.

## Add a Backup Passkey

1. Sign in on a device that already has a working passkey for the account.
2. Open account settings once the UI exposes key management.
3. Choose "add backup passkey" and complete `navigator.credentials.create()` on
   the second device or synced passkey provider.
4. Submit the new key to `POST /v1/accounts/{id}/keys` with:
   - `auth_scheme: "webauthn"`
   - the compressed P256 public key
   - the WebAuthn registration payload
   - the credential id stored client-side for future `credentials.get()` calls
5. Place a small order or cancel an order from the backup device to verify the
   assertion path before relying on it.

Until the settings UI exists, an operator can perform step 4 manually with the
same register-key endpoint after the user completes WebAuthn creation in a
trusted client build.

## Lost Device

If at least one synced or backup passkey remains available, sign in with that
passkey and add a replacement backup key. If all passkeys for the account are
lost, the account cannot sign new orders, cancels, or withdrawals. There is no
seed phrase or server-side reset path by design.

## Real-Device Test Matrix

Before enabling passkeys in production, test:

- iOS Safari: create passkey, reload, sign in, place order, cancel order.
- Android Chrome: create passkey, reload, sign in, place order, cancel order.
- macOS Safari and Chrome: create passkey, reload, sign in, place order, cancel order.
- Windows Edge/Chrome with Windows Hello: create passkey, reload, sign in, place order, cancel order.
- Cross-device backup: add a second passkey, then verify both devices can sign
  order and cancel payloads with strictly increasing nonces.
