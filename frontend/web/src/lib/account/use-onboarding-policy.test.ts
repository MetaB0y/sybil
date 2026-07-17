import { describe, expect, it } from "vitest";
import { demoGrantCopy } from "./use-onboarding-policy";

describe("demoGrantCopy", () => {
  it("shows the server-selected grant exactly", () => {
    expect(demoGrantCopy("1000000000000")).toBe(
      "Every new demo account receives $1000 in play money.",
    );
  });

  it("does not imply funding when onboarding grants zero", () => {
    expect(demoGrantCopy("0")).toBe("New demo accounts currently start with $0.");
  });
});
