import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { deriveOpenBatchPrice, OpenBatchReadNotice } from "./batch-hero";

describe("BatchHero open-batch states", () => {
  it("labels the committed fallback instead of calling it indicative", () => {
    expect(
      deriveOpenBatchPrice(null, "unavailable", 99_000_000n, "Yes"),
    ).toEqual({
      kind: "last-clearing",
      valueNanos: 99_000_000n,
      secondary: "open batch unavailable",
    });
  });

  it("distinguishes a valid no-cross batch from a live indicative solve", () => {
    expect(
      deriveOpenBatchPrice(
        {
          uniquePlacers: 1,
          indicativeYesPriceNanos: 990_000_000n,
          indicativeVolumeNanos: 0n,
        },
        "ready",
        400_000_000n,
        "Yes",
      ),
    ).toMatchObject({
      kind: "last-clearing",
      valueNanos: 400_000_000n,
      secondary: "no open-batch cross",
    });

    expect(
      deriveOpenBatchPrice(
        {
          uniquePlacers: 2,
          indicativeYesPriceNanos: 550_000_000n,
          indicativeVolumeNanos: 10_000_000_000n,
        },
        "ready",
        400_000_000n,
        "Yes",
      ),
    ).toMatchObject({
      kind: "indicative",
      valueNanos: 550_000_000n,
      secondary: "for Yes",
    });
  });

  it("makes cold and cached failures explicit and retryable", () => {
    const unavailable = renderToStaticMarkup(
      <OpenBatchReadNotice
        state="unavailable"
        retrying={false}
        onRetry={() => {}}
      />,
    );
    const stale = renderToStaticMarkup(
      <OpenBatchReadNotice state="stale" retrying onRetry={() => {}} />,
    );

    expect(unavailable).toContain('role="alert"');
    expect(unavailable).toContain("open-batch data unavailable");
    expect(unavailable).toContain(">retry<");
    expect(stale).toContain('role="status"');
    expect(stale).toContain("showing saved response");
    expect(stale).toContain("disabled");
  });
});
