import { afterEach, describe, expect, it, vi } from "vitest";
import {
  accountIdFromUserHandle,
  base64UrlDecode,
  base64UrlEncode,
  createPasskeyForAccount,
  discoverPasskeyAccount,
} from "../webauthn";

afterEach(() => {
  vi.unstubAllGlobals();
  delete process.env.NEXT_PUBLIC_WEBAUTHN_RP_ID;
});

describe("webauthn base64url helpers", () => {
  it("round-trips unpadded base64url bytes", () => {
    const bytes = new Uint8Array([0, 1, 2, 253, 254, 255]);
    const encoded = base64UrlEncode(bytes);

    expect(encoded).toBe("AAEC_f7_");
    expect(base64UrlDecode(encoded)).toEqual(bytes);
  });
});

describe("accountIdFromUserHandle", () => {
  it("decodes an 8-byte big-endian account id", () => {
    const backing = new Uint8Array(10);
    new DataView(backing.buffer).setBigUint64(1, 109n, false);

    expect(accountIdFromUserHandle(backing.subarray(1, 9))).toBe(109);
  });

  it("rejects malformed and unsafe account ids", () => {
    expect(() => accountIdFromUserHandle(new Uint8Array(7))).toThrow(
      "invalid Sybil account id",
    );

    const unsafe = new Uint8Array(8);
    new DataView(unsafe.buffer).setBigUint64(
      0,
      BigInt(Number.MAX_SAFE_INTEGER) + 1n,
      false,
    );
    expect(() => accountIdFromUserHandle(unsafe)).toThrow(
      "too large for this browser",
    );
  });
});

describe("createPasskeyForAccount", () => {
  function installWebAuthn(create: ReturnType<typeof vi.fn>) {
    vi.stubGlobal("window", { PublicKeyCredential: class {} });
    vi.stubGlobal("navigator", { credentials: { create } });
  }

  it("prefers the platform authenticator for onboarding", async () => {
    const create = vi.fn().mockResolvedValue(null);
    installWebAuthn(create);

    await expect(createPasskeyForAccount(109)).rejects.toThrow(
      "Passkey creation was cancelled",
    );
    expect(
      create.mock.calls[0]?.[0].publicKey.authenticatorSelection,
    ).toMatchObject({
      authenticatorAttachment: "platform",
      residentKey: "preferred",
      userVerification: "required",
    });
  });

  it("allows a different authenticator or provider for a backup", async () => {
    const create = vi.fn().mockResolvedValue(null);
    installWebAuthn(create);

    await expect(
      createPasskeyForAccount(109, { authenticatorAttachment: "any" }),
    ).rejects.toThrow("Passkey creation was cancelled");
    const selection =
      create.mock.calls[0]?.[0].publicKey.authenticatorSelection;
    expect(selection).not.toHaveProperty("authenticatorAttachment");
    expect(selection).toMatchObject({
      residentKey: "preferred",
      userVerification: "required",
    });
  });
});

describe("discoverPasskeyAccount", () => {
  function installWebAuthn(get: ReturnType<typeof vi.fn>) {
    vi.stubGlobal("window", { PublicKeyCredential: class {} });
    vi.stubGlobal("navigator", { credentials: { get } });
  }

  it("requests a discoverable credential and recovers its account", async () => {
    process.env.NEXT_PUBLIC_WEBAUTHN_RP_ID = "app.sybil.test";
    const userHandle = new Uint8Array(8);
    new DataView(userHandle.buffer).setBigUint64(0, 109n, false);
    const rawId = new Uint8Array([1, 2, 253, 254]);
    const get = vi.fn().mockResolvedValue({
      rawId: rawId.buffer,
      response: { userHandle: userHandle.buffer },
    });
    installWebAuthn(get);

    await expect(discoverPasskeyAccount()).resolves.toEqual({
      accountId: 109,
      credentialIdB64url: "AQL9_g",
    });
    expect(get).toHaveBeenCalledOnce();
    expect(get.mock.calls[0]?.[0]).toMatchObject({
      publicKey: {
        allowCredentials: [],
        userVerification: "required",
        rpId: "app.sybil.test",
        timeout: 60_000,
      },
    });
    expect(get.mock.calls[0]?.[0].publicKey.challenge).toHaveLength(32);
  });

  it("explains how to recover a legacy passkey without a user handle", async () => {
    const get = vi.fn().mockResolvedValue({
      rawId: new Uint8Array([1]).buffer,
      response: { userHandle: null },
    });
    installWebAuthn(get);

    await expect(discoverPasskeyAccount()).rejects.toThrow(
      "This passkey predates usernameless login; use the same browser it was created in",
    );
  });
});
