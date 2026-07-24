"use client";

import { createPasskeyForAccount } from "@/lib/auth/webauthn";
import { registerPasskey, SettingsActionError } from "./settings";

const MAX_STALE_BINDING_ATTEMPTS = 3;

export interface AddBackupPasskeyArgs {
  accountId: number;
  publicKeyHex: string;
  credentialIdB64url: string;
}

/**
 * Create and register a second passkey, authorized by the active passkey.
 *
 * The new credential never replaces the current browser session. It becomes an
 * additional active signing key, so either passkey can subsequently recover
 * the account through discoverable sign-in.
 */
export async function addBackupPasskey(
  args: AddBackupPasskeyArgs,
): Promise<{ publicKeyHex: string }> {
  const passkey = await createPasskeyForAccount(args.accountId, {
    authenticatorAttachment: "any",
  });
  for (let attempt = 0; attempt < MAX_STALE_BINDING_ATTEMPTS; attempt += 1) {
    try {
      await registerPasskey({
        accountId: args.accountId,
        publicKeyHex: args.publicKeyHex,
        authScheme: "webauthn",
        credentialIdB64url: args.credentialIdB64url,
        passkey,
        label: "backup passkey",
      });
      return { publicKeyHex: passkey.publicKeyHex };
    } catch (error) {
      const staleBinding =
        error instanceof SettingsActionError &&
        error.status === 409 &&
        error.message.includes("stale key-operation state binding");
      if (!staleBinding || attempt === MAX_STALE_BINDING_ATTEMPTS - 1) {
        throw error;
      }
    }
  }
  return { publicKeyHex: passkey.publicKeyHex };
}
