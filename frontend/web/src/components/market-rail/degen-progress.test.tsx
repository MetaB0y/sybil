import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { DegenProgress } from "./degen-progress";

const common = {
  side: "YES" as const,
  secondsLeft: 24,
  timeProgress01: 0.4,
  filledQty: 12n,
  targetQty: 20n,
  limitPriceNanos: 540_000_000n,
  avgPriceNanos: 530_000_000n,
  onBetAgain: () => {},
};

describe("DegenProgress", () => {
  it("shows the countdown and fill meter while tracking", () => {
    const html = renderToStaticMarkup(
      <DegenProgress {...common} phase="tracking" />,
    );
    expect(html).toMatch(/FILLING/i);
    expect(html).toContain("24s");
    expect(html).toContain("12");
    expect(html).toContain("20");
    expect(html).not.toMatch(/Bet again/i);
  });

  it("shows a filled result with avg price and a reset", () => {
    const html = renderToStaticMarkup(
      <DegenProgress {...common} phase="filled" filledQty={20n} />,
    );
    expect(html).toMatch(/FILLED/i);
    expect(html).toMatch(/Bet again/i);
  });

  it("shows a partial result", () => {
    const html = renderToStaticMarkup(
      <DegenProgress {...common} phase="partial" />,
    );
    expect(html).toMatch(/PARTIAL/i);
    expect(html).toContain("12");
    expect(html).toMatch(/Bet again/i);
  });

  it("shows a no-fill result", () => {
    const html = renderToStaticMarkup(
      <DegenProgress {...common} phase="none" filledQty={0n} avgPriceNanos={null} />,
    );
    expect(html).toMatch(/NO FILL/i);
    expect(html).toMatch(/Bet again/i);
  });
});
