import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { DegenProgress } from "./degen-progress";

const common = {
  side: "YES" as const,
  secondsLeft: 24,
  timeProgress01: 0.4,
  filledQty: 12n,
  targetQty: 20n,
  betUsd: 10,
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

  it("shows a filled result with the bet amount + side and a reset", () => {
    const html = renderToStaticMarkup(
      <DegenProgress {...common} phase="filled" filledQty={20n} />,
    );
    expect(html).toMatch(/Successfully bet/i);
    expect(html).not.toMatch(/Congratulations/i);
    expect(html).toContain("$10");
    expect(html).toContain("YES");
    expect(html).toMatch(/Bet again/i);
  });

  it("colours success green even on a NO bet, and a miss red", () => {
    const filledNo = renderToStaticMarkup(
      <DegenProgress {...common} side="NO" phase="filled" filledQty={20n} />,
    );
    expect(filledNo).toContain("var(--yes)");
    expect(filledNo).not.toContain("color:var(--no)");

    const missedYes = renderToStaticMarkup(
      <DegenProgress {...common} side="YES" phase="none" filledQty={0n} />,
    );
    expect(missedYes).toContain("var(--no)");
  });

  it("shows a partial result with the filled dollars out of the stake", () => {
    // 12 of 20 shares of a $10 bet -> $6 out of $10.
    const html = renderToStaticMarkup(
      <DegenProgress {...common} phase="partial" />,
    );
    expect(html).toMatch(/Half in/i);
    expect(html).toContain("$6");
    expect(html).toContain("$10");
    expect(html).toMatch(/Bet again/i);
  });

  it("shows a no-fill result", () => {
    const html = renderToStaticMarkup(
      <DegenProgress {...common} phase="none" filledQty={0n} />,
    );
    expect(html).toMatch(/failed/i);
    expect(html).toMatch(/Bet again/i);
  });
});
