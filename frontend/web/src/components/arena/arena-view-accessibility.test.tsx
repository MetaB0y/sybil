import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { ArenaFilterSelect } from "./arena-view";

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
