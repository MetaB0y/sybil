import { describe, expect, it, vi } from "vitest";
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

  it("distinguishes loading, failed backfill, and a real empty chain", () => {
    const loading = renderToStaticMarkup(
      <BatchesTable rows={[]} isBackfilling />,
    );
    const failed = renderToStaticMarkup(
      <BatchesTable
        rows={[]}
        isBackfilling={false}
        backfillError
        onRetry={vi.fn()}
      />,
    );
    const empty = renderToStaticMarkup(
      <BatchesTable rows={[]} isBackfilling={false} />,
    );

    expect(loading).toContain('role="status"');
    expect(loading).toContain("loading recent batches…");
    expect(failed).toContain('role="alert"');
    expect(failed).toContain("not shown as an empty chain");
    expect(failed).not.toContain("no batches yet");
    expect(empty).toContain(
      "no batches yet — waiting for the first committed batch",
    );
    const liveToggle = empty.match(/<button[^>]*aria-pressed="true"[^>]*>/)?.[0];
    expect(liveToggle).toContain("disabled");
  });

  it("keeps live rows visible when historical backfill refresh fails", () => {
    const html = renderToStaticMarkup(
      <BatchesTable
        rows={[row]}
        isBackfilling={false}
        backfillError
        retrying
        onRetry={vi.fn()}
      />,
    );

    expect(html).toContain('role="status"');
    expect(html).toContain("showing live and saved rows");
    expect(html).toContain('id="activity-batch-42-trigger"');
    expect(html).toContain("disabled");
  });
});
