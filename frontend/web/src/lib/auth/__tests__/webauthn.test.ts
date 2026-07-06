import { describe, expect, it } from "vitest";
import { base64UrlDecode, base64UrlEncode } from "../webauthn";

describe("webauthn base64url helpers", () => {
  it("round-trips unpadded base64url bytes", () => {
    const bytes = new Uint8Array([0, 1, 2, 253, 254, 255]);
    const encoded = base64UrlEncode(bytes);

    expect(encoded).toBe("AAEC_f7_");
    expect(base64UrlDecode(encoded)).toEqual(bytes);
  });
});
