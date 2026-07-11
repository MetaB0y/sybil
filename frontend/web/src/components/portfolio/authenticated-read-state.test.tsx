import { describe, expect, it } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import {
  AuthenticatedReadState,
  authenticatedReadStatus,
} from "./authenticated-read-state";

describe("authenticated portfolio read states", () => {
  it("treats successful empty reads as ready", () => {
    expect(
      authenticatedReadStatus([
        { isPending: false, error: null },
        { isPending: false, error: null },
      ]),
    ).toBe("ready");
  });

  it("keeps pending and failed reads out of the ready state", () => {
    expect(authenticatedReadStatus([{ isPending: true, error: null }])).toBe(
      "loading",
    );
    expect(
      authenticatedReadStatus([
        { isPending: true, error: null },
        { isPending: false, error: new Error("private read failed") },
      ]),
    ).toBe("error");
  });

  it("renders accessible loading and retryable error feedback", () => {
    const loading = renderToStaticMarkup(
      <AuthenticatedReadState
        status="loading"
        title="Loading your portfolio"
        message="Checking your private balances."
      />,
    );
    expect(loading).toContain('role="status"');
    expect(loading).toContain('aria-busy="true"');
    expect(loading).not.toContain("Retry");

    const failed = renderToStaticMarkup(
      <AuthenticatedReadState
        status="error"
        title="Portfolio unavailable"
        message="Private account data could not be loaded."
        onRetry={() => undefined}
      />,
    );
    expect(failed).toContain('role="alert"');
    expect(failed).toContain(">Retry</button>");
    expect(failed).not.toContain("$0");
  });
});
