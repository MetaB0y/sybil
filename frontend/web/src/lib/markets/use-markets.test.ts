import { describe, expect, it } from "vitest";
import {
  assemble,
  eventVisibleOnIndex,
  isClosed,
  type Market,
} from "./use-markets";

function mk(partial: Partial<Market> & { market_id: number }): Market {
  return {
    name: `m${partial.market_id}`,
    status: "active",
    ...partial,
  } as Market;
}

describe("markets/use-markets helpers", () => {
  it("isClosed only true for explicit closed===true", () => {
    expect(isClosed(mk({ market_id: 1, closed: true }))).toBe(true);
    expect(isClosed(mk({ market_id: 2, closed: false }))).toBe(false);
    expect(isClosed(mk({ market_id: 3 }))).toBe(false);
  });

  it("eventVisibleOnIndex hides only when every market is closed", () => {
    expect(
      eventVisibleOnIndex([
        mk({ market_id: 1, closed: true }),
        mk({ market_id: 2, closed: false }),
      ]),
    ).toBe(true);
    expect(
      eventVisibleOnIndex([
        mk({ market_id: 1, closed: true }),
        mk({ market_id: 2, closed: true }),
      ]),
    ).toBe(false);
  });

  it("assemble keeps closed markets in byId and groups", () => {
    const bundle = assemble([
      mk({ market_id: 1, event_id: "e1", event_title: "E1", closed: true }),
      mk({ market_id: 2, event_id: "e1", event_title: "E1", closed: false }),
      mk({ market_id: 3, closed: true }),
    ]);
    expect(bundle.byId.has(1)).toBe(true); // closed retained
    expect(bundle.byId.has(3)).toBe(true);
    const e1 = bundle.groups.find((g) => g.eventId === "e1");
    expect(e1?.markets.length).toBe(2); // both, incl. closed
    expect(bundle.total).toBe(3);
  });
});
