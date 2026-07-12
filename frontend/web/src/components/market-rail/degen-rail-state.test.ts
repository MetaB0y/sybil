import { describe, expect, it } from "vitest";
import { degenCtaState } from "./degen-rail";

const ready = {
  connected: true,
  latestBatchReady: true,
  signing: false,
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
    expect(degenCtaState({ ...ready, orderReady: false })).toBe("raise_bet");
    expect(degenCtaState({ ...ready, insufficient: true })).toBe(
      "insufficient",
    );
    expect(degenCtaState(ready)).toBe("ready");
  });
});
