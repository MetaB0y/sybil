import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vitest";
import { deriveNavSearchDataState, NavSearchDataNotice } from "./nav-search";

describe("NavSearch data truthfulness", () => {
  it("distinguishes loading and unavailable data from real search results", () => {
    expect(
      deriveNavSearchDataState({
        hasBundle: false,
        hasError: false,
      }),
    ).toBe("loading");
    expect(
      deriveNavSearchDataState({
        hasBundle: false,
        hasError: true,
      }),
    ).toBe("unavailable");
    expect(
      deriveNavSearchDataState({
        hasBundle: true,
        hasError: true,
      }),
    ).toBe("stale");
  });

  it("renders accessible loading and retryable failure notices", () => {
    const loading = renderToStaticMarkup(
      <NavSearchDataNotice
        state="loading"
        retrying={false}
        onRetry={vi.fn()}
      />,
    );
    const failed = renderToStaticMarkup(
      <NavSearchDataNotice
        state="unavailable"
        retrying={false}
        onRetry={vi.fn()}
      />,
    );
    const retrying = renderToStaticMarkup(
      <NavSearchDataNotice state="unavailable" retrying onRetry={vi.fn()} />,
    );
    const stale = renderToStaticMarkup(
      <NavSearchDataNotice state="stale" retrying={false} onRetry={vi.fn()} />,
    );

    expect(loading).toContain('role="status"');
    expect(loading).toContain("loading market search…");
    expect(failed).toContain('role="alert"');
    expect(failed).toContain("market search unavailable");
    expect(failed).toContain(">retry</button>");
    expect(retrying).toContain("disabled");
    expect(retrying).toContain("retrying…");
    expect(stale).toContain('role="status"');
    expect(stale).toContain(
      "market search update failed · showing saved results",
    );
    expect(stale).toContain(">retry</button>");
  });
});
