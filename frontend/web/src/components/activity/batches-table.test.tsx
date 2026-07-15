import { beforeEach, describe, expect, it, vi } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";

import { BatchesTable } from "./batches-table";
import type { BatchRow } from "@/lib/activity/types";

const batches = vi.hoisted(() => ({
  rows: [] as BatchRow[],
  isLoading: false,
  hasOlder: false,
  error: null as Error | null,
  isRetrying: false,
  retry: vi.fn(),
}));

vi.mock("@/lib/activity/use-batches", () => ({
  useBatchPage: () => batches,
}));

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
  beforeEach(() => {
    batches.rows = [];
    batches.isLoading = false;
    batches.hasOlder = false;
    batches.error = null;
    batches.isRetrying = false;
    batches.retry.mockReset();
  });

  it("renders each expandable row as a native disclosure button", () => {
    batches.rows = [row];
    const html = renderToStaticMarkup(
      <BatchesTable renderDetail={() => <p>Batch detail</p>} />,
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
    batches.rows = [row];
    const html = renderToStaticMarkup(<BatchesTable />);

    expect(html).toContain("select any row to expand");
    expect(html).not.toContain("click any row");
  });

  it("distinguishes loading, failed backfill, and a real empty chain", () => {
    batches.isLoading = true;
    const loading = renderToStaticMarkup(<BatchesTable />);
    batches.isLoading = false;
    batches.error = new Error("unavailable");
    const failed = renderToStaticMarkup(<BatchesTable />);
    batches.error = null;
    const empty = renderToStaticMarkup(<BatchesTable />);

    expect(loading).toContain('role="status"');
    expect(loading).toContain("loading recent batches…");
    expect(failed).toContain('role="alert"');
    expect(failed).toContain("not shown as an empty chain");
    expect(failed).not.toContain("no batches yet");
    expect(empty).toContain(
      "no batches yet — waiting for the first committed batch",
    );
    const liveToggle = empty.match(
      /<button[^>]*aria-pressed="true"[^>]*>/,
    )?.[0];
    expect(liveToggle).toContain("disabled");
  });

  it("keeps live rows visible when historical backfill refresh fails", () => {
    batches.rows = [row];
    batches.error = new Error("unavailable");
    batches.isRetrying = true;
    const html = renderToStaticMarkup(<BatchesTable />);

    expect(html).toContain('role="status"');
    expect(html).toContain("showing live and saved rows");
    expect(html).toContain('id="activity-batch-42-trigger"');
    expect(html).toContain("disabled");
  });
});
