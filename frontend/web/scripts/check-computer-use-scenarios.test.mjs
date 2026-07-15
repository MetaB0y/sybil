import { describe, expect, test } from "vitest";

import {
  parseScenario,
  validateCorpus,
} from "./check-computer-use-scenarios.mjs";

function validSource(overrides = {}) {
  const values = {
    id: "sample-journey",
    priority: "p0",
    mode: "read-only",
    personas: "visitor",
    routes: "/",
    fixtures: "active-market",
    environments: "desktop",
    ...overrides,
  };
  return `---
id: ${values.id}
priority: ${values.priority}
mode: ${values.mode}
personas: ${values.personas}
routes: ${values.routes}
fixtures: ${values.fixtures}
environments: ${values.environments}
---

# Sample journey

## Intent

Confirm a visible user outcome.

## Preconditions

- Start from a fresh browser profile.

## Steps

1. Open the application.
2. Inspect the visible result.

## Observable assertions

- The result is clear without implementation knowledge.

## Evidence

- Capture the visible result.

## Cleanup

- No product state was changed.

## Stop conditions

- Stop if the fixture is unavailable.
`;
}

describe("computer-use scenario contract", () => {
  test("parses the constrained natural-language format", () => {
    const scenario = parseScenario(validSource(), "sample-journey.md");
    expect(scenario).toMatchObject({
      id: "sample-journey",
      mode: "read-only",
      routes: ["/"],
      title: "Sample journey",
    });
  });

  test("rejects implementation-coupled selectors", () => {
    expect(() =>
      parseScenario(
        validSource().replace(
          "Open the application.",
          "Click data-testid=market.",
        ),
        "sample-journey.md",
      ),
    ).toThrow(/implementation-coupled/);
  });

  test("rejects missing contract sections", () => {
    const source = validSource().replace(
      /## Evidence\n\n- Capture the visible result\.\n\n/,
      "",
    );
    expect(() => parseScenario(source, "sample-journey.md")).toThrow(
      /Evidence/,
    );
  });

  test("rejects extra contract sections", () => {
    const source = validSource().replace(
      "## Stop conditions",
      "## Implementation notes\n\n- Inspect internals.\n\n## Stop conditions",
    );
    expect(() => parseScenario(source, "sample-journey.md")).toThrow(
      /unknown level-two section/,
    );
  });

  test("requires fault scenarios to name an instrumented browser", () => {
    expect(() =>
      parseScenario(
        validSource({ mode: "controlled-fault" }),
        "sample-journey.md",
      ),
    ).toThrow(/instrumented-browser/);
  });

  test("rejects protocol-relative or otherwise external routes", () => {
    expect(() =>
      parseScenario(
        validSource({ routes: "//unexpected.example" }),
        "sample-journey.md",
      ),
    ).toThrow(/local app path/);
  });

  test("requires both safe and disposable P0 coverage in a corpus", () => {
    const readOnly = parseScenario(validSource(), "sample-journey.md");
    expect(() => validateCorpus([readOnly])).toThrow(/disposable-account/);

    const disposable = parseScenario(
      validSource({
        id: "account-journey",
        mode: "disposable-account",
      }).replace("# Sample journey", "# Account journey"),
      "account-journey.md",
    );
    expect(validateCorpus([readOnly, disposable])).toHaveLength(2);
  });
});
