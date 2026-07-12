import { describe, expect, it } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import { L1DepositGuidance } from "./l1-deposit-guide";

describe("L1DepositGuidance", () => {
  it("shows the exact routing key and distinguishes it from signing keys", () => {
    const key = "ab".repeat(32);
    const html = renderToStaticMarkup(
      <L1DepositGuidance accountId={17} sybilAccountKeyHex={key} />,
    );

    expect(html).toContain(`0x${key}`);
    expect(html).toContain("account #17");
    expect(html).toContain("different from your passkey or signing public key");
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
});
