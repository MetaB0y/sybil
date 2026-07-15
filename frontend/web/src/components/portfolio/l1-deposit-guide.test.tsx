import { describe, expect, it } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import {
  BridgeDomainGuidance,
  L1DepositGuidance,
  bridgeChainLabel,
} from "./l1-deposit-guide";

describe("L1DepositGuidance", () => {
  it("shows the exact routing key and distinguishes it from signing keys", () => {
    const key = "ab".repeat(32);
    const html = renderToStaticMarkup(
      <L1DepositGuidance accountId={17} sybilAccountKeyHex={key} />,
    );

    expect(html).toContain(`0x${key}`);
    expect(html).toContain("account #17");
    expect(html).toContain("different from your passkey or signing public key");
    expect(html).toContain("Deposits unavailable");
    expect(html).toContain("Do not send tokens");
  });

  it("gives only implemented quarantine recovery guidance", () => {
    const html = renderToStaticMarkup(
      <L1DepositGuidance
        accountId={17}
        sybilAccountKeyHex={`0x${"cd".repeat(32)}`}
      />,
    );

    expect(html).toContain("committed quarantine ledger");
    expect(html).toContain("any later signing key is registered");
    expect(html).toContain("automatically claims the full");
    expect(html).toContain("aggregate quarantine totals only");
    expect(html).toContain("There is no L1 refund flow today");
    expect(html).not.toContain("Request refund");
  });

  it("shows the complete configured domain without implying wallet or proof safety", () => {
    const html = renderToStaticMarkup(
      <BridgeDomainGuidance
        domain={{
          chain_id: 11_155_111,
          vault_address_hex: "11".repeat(20),
          token_address_hex: "0x" + "22".repeat(20),
        }}
      />,
    );

    expect(html).toContain("Configured bridge domain");
    expect(html).toContain("Sepolia (chain 11155111)");
    expect(html).toContain("0x" + "11".repeat(20));
    expect(html).toContain("0x" + "22".repeat(20));
    expect(html).toContain("No in-browser wallet transaction is available");
    expect(html).toContain(
      "does not attest the token&#x27;s value or verifier safety",
    );
    expect(html).toContain("do not use real funds");
    expect(bridgeChainLabel(1)).toBe("Chain 1");
  });
});
