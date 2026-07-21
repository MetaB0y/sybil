import { describe, expect, it } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";

import { DevnetNotice } from "./devnet-notice";
import { DEVNET_DISMISSED_KEY } from "@/lib/devnet";

describe("DevnetNotice", () => {
  const html = renderToStaticMarkup(<DevnetNotice />);

  it("names the risk in both wordings", () => {
    // The long one is desktop's, the short one a phone's; CSS picks by width,
    // so both have to be in the markup.
    expect(html).toMatch(/restarted and balances, positions and history reset/);
    expect(html).toMatch(/Restarts and data loss are possible/);
    expect(html).toMatch(/functionality is limited/i);
  });

  it("renders server-side so the strip never pops in after hydration", () => {
    expect(html).toContain("devnet-notice");
  });

  it("offers a labelled dismiss control", () => {
    expect(html).toContain('aria-label="Dismiss devnet notice"');
  });

  it("keys its dismissal on the same string the pre-paint script reads", () => {
    // layout.tsx interpolates this constant into its init script. It lives in
    // a plain module because a value imported across a "use client" boundary
    // reaches a server component as a client reference, not a string.
    expect(DEVNET_DISMISSED_KEY).toBe("sybil-devnet-notice-dismissed");
  });
});
