import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { AccountKeyPanel } from "./account-key-section";

describe("AccountKeyPanel", () => {
  it("renders the key 0x-prefixed and names the account", () => {
    const html = renderToStaticMarkup(
      <AccountKeyPanel accountId={17} sybilAccountKeyHex={"ab".repeat(32)} />,
    );
    expect(html).toContain(`0x${"ab".repeat(32)}`);
    expect(html).toContain("account #17");
  });

  it("does not double-prefix a key that already carries 0x", () => {
    const key = `0x${"cd".repeat(32)}`;
    const html = renderToStaticMarkup(
      <AccountKeyPanel accountId={3} sybilAccountKeyHex={key} />,
    );
    expect(html).toContain(key);
    expect(html).not.toContain("0x0x");
  });

  it("says the key is public so nobody mistakes it for a secret", () => {
    const html = renderToStaticMarkup(
      <AccountKeyPanel accountId={1} sybilAccountKeyHex={"01".repeat(32)} />,
    );
    expect(html).toContain("public and safe to share");
  });
});
