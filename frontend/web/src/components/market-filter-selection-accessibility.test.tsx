import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vitest";
import { CategoryTabs } from "./category-tabs";
import { MarketsFilterBar } from "./markets-filter-bar";

vi.mock("next/navigation", () => ({
  usePathname: () => "/",
  useRouter: () => ({ replace: vi.fn() }),
  useSearchParams: () => new URLSearchParams("category=Politics"),
}));

describe("market filter selection accessibility", () => {
  it("announces the selected category", () => {
    const html = renderToStaticMarkup(
      <QueryClientProvider client={new QueryClient()}>
        <CategoryTabs />
      </QueryClientProvider>,
    );

    expect(html).toMatch(/aria-pressed="true"[^>]*>Politics/);
    expect(html).toMatch(/aria-pressed="false"[^>]*>All<\/button>/);
  });

  it("announces the selected sort and closed-market filter", () => {
    const html = renderToStaticMarkup(
      <QueryClientProvider client={new QueryClient()}>
        <MarketsFilterBar
          sort="traders"
          onSortChange={() => undefined}
          hideClosed={true}
          onHideClosedChange={() => undefined}
        />
      </QueryClientProvider>,
    );

    expect(html).toContain('role="group" aria-label="market sorting"');
    expect(html).toMatch(
      /role="group" aria-label="market sorting"[^>]*>.*Volume.*Traders.*<\/div><span/,
    );
    expect(html).not.toContain(">New</button>");
    expect(html).toMatch(/aria-pressed="true"[^>]*>Traders<\/button>/);
    expect(html).toMatch(/aria-pressed="false"[^>]*>Volume<\/button>/);
    expect(html).toMatch(/aria-pressed="true"[^>]*>Hide closed<\/button>/);
  });
});
