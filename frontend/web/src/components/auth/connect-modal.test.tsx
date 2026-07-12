import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { ImportTab } from "./connect-modal";

describe("connect modal accessibility", () => {
  it("programmatically labels the existing-account credentials", () => {
    const html = renderToStaticMarkup(<ImportTab />);

    expect(html).toContain('<label for="connect-import-account-id"');
    expect(html).toContain('id="connect-import-account-id" type="text"');
    expect(html).toContain('<label for="connect-import-private-key"');
    expect(html).toContain('id="connect-import-private-key"');
  });
});
