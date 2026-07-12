import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  createPasskeyForAccount: vi.fn(),
  registerPasskey: vi.fn(),
}));

vi.mock("@/lib/auth/webauthn", () => ({
  createPasskeyForAccount: mocks.createPasskeyForAccount,
}));

vi.mock("./settings", () => ({
  registerPasskey: mocks.registerPasskey,
}));

import { addBackupPasskey } from "./backup-passkey";

describe("addBackupPasskey", () => {
  beforeEach(() => vi.clearAllMocks());

  it("creates a credential and registers it with the active passkey", async () => {
    const passkey = {
      publicKeyHex: "03backup",
      credentialIdB64url: "backup-credential",
      attestationObjectB64url: "attestation",
      clientDataJSONB64url: "client-data",
    };
    mocks.createPasskeyForAccount.mockResolvedValue(passkey);
    mocks.registerPasskey.mockResolvedValue(undefined);

    await expect(
      addBackupPasskey({
        accountId: 17,
        publicKeyHex: "02primary",
        credentialIdB64url: "primary-credential",
      }),
    ).resolves.toEqual({ publicKeyHex: "03backup" });

    expect(mocks.createPasskeyForAccount).toHaveBeenCalledWith(17, {
      authenticatorAttachment: "any",
    });
    expect(mocks.registerPasskey).toHaveBeenCalledWith({
      accountId: 17,
      publicKeyHex: "02primary",
      authScheme: "webauthn",
      credentialIdB64url: "primary-credential",
      passkey,
      label: "backup passkey",
    });
  });

  it("does not attempt registration when credential creation fails", async () => {
    mocks.createPasskeyForAccount.mockRejectedValue(
      new DOMException("cancelled", "NotAllowedError"),
    );

    await expect(
      addBackupPasskey({
        accountId: 17,
        publicKeyHex: "02primary",
        credentialIdB64url: "primary-credential",
      }),
    ).rejects.toMatchObject({ name: "NotAllowedError" });
    expect(mocks.registerPasskey).not.toHaveBeenCalled();
  });
});
