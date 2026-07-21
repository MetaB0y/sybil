import { describe, expect, it } from "vitest";
import MarketsPageClient from "./markets-page-client";
import MarketsPage from "./page";

describe("markets index route", () => {
  it("renders a static client-fetch shell without server catalog props", () => {
    const page = MarketsPage();

    expect(page.type).toBe(MarketsPageClient);
    expect(page.props).toEqual({});
  });
});
