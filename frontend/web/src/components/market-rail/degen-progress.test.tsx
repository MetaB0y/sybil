import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { DegenProgress } from "./degen-progress";

const common = {
  side: "YES" as const,
  secondsLeft: 24,
  timeProgress01: 0.4,
  filledQty: 12n,
  targetQty: 20n,
  onBetAgain: () => {},
};

describe("DegenProgress", () => {
  it("shows the countdown and fill meter while tracking", () => {
    const html = renderToStaticMarkup(
      <DegenProgress {...common} phase="tracking" />,
    );
    expect(html).toMatch(/Placing your bet/i);
    expect(html).toContain("24s");
    expect(html).toContain("12");
    expect(html).toContain("20");
    expect(html).not.toMatch(/Bet again/i);
  });

  it("shows a filled result and a reset", () => {
    const html = renderToStaticMarkup(
      <DegenProgress {...common} phase="filled" filledQty={20n} />,
    );
    expect(html).toMatch(/Bet placed/i);
    expect(html).toMatch(/Bet again/i);
  });

  it("shows a partial result", () => {
    const html = renderToStaticMarkup(
      <DegenProgress {...common} phase="partial" />,
    );
    expect(html).toMatch(/Half in/i);
    expect(html).toContain("12");
    expect(html).toMatch(/Bet again/i);
  });

  it("shows a no-fill result", () => {
    const html = renderToStaticMarkup(
      <DegenProgress {...common} phase="none" filledQty={0n} />,
    );
    expect(html).toMatch(/Missed/i);
    expect(html).toMatch(/Bet again/i);
  });
});
