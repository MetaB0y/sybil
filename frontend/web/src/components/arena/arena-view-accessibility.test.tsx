import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vitest";
import type { ArenaFeed } from "@/lib/arena/use-arena-feed";
import {
  arenaFeedUiState,
  arenaPanelDataState,
  ArenaFeedGate,
  ArenaFilterSelect,
  ArenaPanelDataNotice,
} from "./arena-view";

describe("ArenaFilterSelect", () => {
  it("gives a dashboard filter an explicit accessible name", () => {
    const html = renderToStaticMarkup(
      <ArenaFilterSelect label="Filter recent decisions by bot" defaultValue="">
        <option value="">All bots</option>
        <option value="alice">alice</option>
      </ArenaFilterSelect>,
    );

    expect(html).toContain('aria-label="Filter recent decisions by bot"');
    expect(html).toContain("All bots");
  });
});

describe("Arena feed truthfulness", () => {
  const dashboard = <div>dashboard zero sentinel</div>;

  it("shows a loading status without rendering fake zero totals", () => {
    const state = arenaFeedUiState({
      data: undefined,
      isPending: true,
      isError: false,
    });
    const html = renderToStaticMarkup(
      <ArenaFeedGate state={state} retrying={false} onRetry={vi.fn()}>
        {dashboard}
      </ArenaFeedGate>,
    );

    expect(html).toContain('role="status"');
    expect(html).toContain("Loading Arena");
    expect(html).not.toContain("dashboard zero sentinel");
  });

  it("surfaces a transport error with retry and no invented data", () => {
    const state = arenaFeedUiState({
      data: undefined,
      isPending: false,
      isError: true,
    });
    const html = renderToStaticMarkup(
      <ArenaFeedGate state={state} retrying={false} onRetry={vi.fn()}>
        {dashboard}
      </ArenaFeedGate>,
    );

    expect(html).toContain('role="alert"');
    expect(html).toContain("Arena feed unavailable");
    expect(html).toContain("Retry Arena feed");
    expect(html).not.toContain("dashboard zero sentinel");
  });

  it("keeps the last successful snapshot visible after a refresh failure", () => {
    const state = arenaFeedUiState({
      data: { db_available: true } as ArenaFeed,
      isPending: false,
      isError: true,
    });
    const html = renderToStaticMarkup(
      <ArenaFeedGate state={state} retrying={false} onRetry={vi.fn()}>
        {dashboard}
      </ArenaFeedGate>,
    );

    expect(html).toContain("Arena refresh failed");
    expect(html).toContain("dashboard zero sentinel");
  });

  it("distinguishes an authoritative database failure from empty activity", () => {
    const state = arenaFeedUiState({
      data: {
        db_available: false,
        error: "decision database cannot be opened",
      } as ArenaFeed,
      isPending: false,
      isError: false,
    });
    const html = renderToStaticMarkup(
      <ArenaFeedGate state={state} retrying={false} onRetry={vi.fn()}>
        {dashboard}
      </ArenaFeedGate>,
    );

    expect(html).toContain("Arena database unavailable");
    expect(html).toContain("decision database cannot be opened");
    expect(html).not.toContain("dashboard zero sentinel");
  });
});

describe("Arena panel history truthfulness", () => {
  it("disables retry while a failed history request is retrying", () => {
    const state = arenaPanelDataState({
      data: undefined,
      enabled: true,
      isPending: false,
      isError: true,
      label: "Equity history",
    });
    const html = renderToStaticMarkup(
      <ArenaPanelDataNotice state={state} retrying={true} onRetry={vi.fn()} />,
    );

    expect(html).toContain("Retrying…");
    expect(html).toContain("disabled");
  });
});
