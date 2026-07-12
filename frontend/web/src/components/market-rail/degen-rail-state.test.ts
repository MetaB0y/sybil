import { describe, expect, it } from "vitest";
import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { DegenCancelAlert, degenCtaState } from "./degen-rail";

const ready = {
  connected: true,
  latestBatchReady: true,
  signing: false,
  balanceKnown: true,
  balancePending: false,
  orderReady: true,
  insufficient: false,
};

describe("degenCtaState", () => {
  it("keeps the connect action available without an account", () => {
    expect(degenCtaState({ ...ready, connected: false })).toBe("connect");
  });

  it("blocks a connected bet until the latest batch is hydrated", () => {
    expect(degenCtaState({ ...ready, latestBatchReady: false })).toBe(
      "waiting_batch",
    );
  });

  it("distinguishes signing, invalid size, balance, and ready states", () => {
    expect(degenCtaState({ ...ready, signing: true })).toBe("signing");
    expect(
      degenCtaState({ ...ready, balanceKnown: false, balancePending: true }),
    ).toBe("waiting_balance");
    expect(
      degenCtaState({ ...ready, balanceKnown: false, balancePending: false }),
    ).toBe("balance_unavailable");
    expect(degenCtaState({ ...ready, orderReady: false })).toBe("raise_bet");
    expect(degenCtaState({ ...ready, insufficient: true })).toBe(
      "insufficient",
    );
    expect(degenCtaState(ready)).toBe("ready");
  });

  it("renders cancellation failures as an accessible alert", () => {
    const html = renderToStaticMarkup(
      createElement(DegenCancelAlert, {
        message: "Your bet may still be active.",
      }),
    );
    expect(html).toContain('role="alert"');
    expect(html).toContain("Your bet may still be active.");
    expect(
      renderToStaticMarkup(createElement(DegenCancelAlert, { message: null })),
    ).toBe("");
  });
});
