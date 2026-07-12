"use client";

import { createPasskeyForAccount } from "@/lib/auth/webauthn";
import { registerPasskey } from "./settings";

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
  await registerPasskey({
    accountId: args.accountId,
    publicKeyHex: args.publicKeyHex,
    authScheme: "webauthn",
    credentialIdB64url: args.credentialIdB64url,
    passkey,
    label: "backup passkey",
  });
  return { publicKeyHex: passkey.publicKeyHex };
}
