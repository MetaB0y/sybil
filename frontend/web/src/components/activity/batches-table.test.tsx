import { describe, expect, it } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";

import { BatchesTable } from "./batches-table";
import type { BatchRow } from "@/lib/activity/types";

const row: BatchRow = {
  height: 42,
  timestampMs: Date.UTC(2026, 6, 11, 12, 0, 0),
  matchedVolumeNanos: 12_000_000_000n,
  welfareNanos: 750_000_000n,
  ordersPlaced: 8,
  ordersMatched: 6,
  ordersUnmatched: 2,
  marketsTouched: 3,
  uniqueTraders: 4,
};

describe("BatchesTable", () => {
  it("renders each expandable row as a native disclosure button", () => {
    const html = renderToStaticMarkup(
      <BatchesTable
        rows={[row]}
        isBackfilling={false}
        renderDetail={() => <p>Batch detail</p>}
      />,
    );

    const rowButton = html.match(
      /<button[^>]*class="activity-batch-row"[^>]*>/,
    )?.[0];
    expect(rowButton).toBeDefined();
    expect(rowButton).toContain('type="button"');
    expect(rowButton).toContain('aria-expanded="false"');
    expect(rowButton).toContain('aria-controls="activity-batch-42-detail"');
    expect(rowButton).toContain('id="activity-batch-42-trigger"');
    expect(rowButton).not.toContain("aria-label");
  });

  it("uses input-neutral copy for mouse, keyboard, and touch users", () => {
    const html = renderToStaticMarkup(
      <BatchesTable rows={[row]} isBackfilling={false} />,
    );

    expect(html).toContain("select any row to expand");
    expect(html).not.toContain("click any row");
  });
});
