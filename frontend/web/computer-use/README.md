# Computer-use acceptance

This directory is the tool-independent acceptance layer for Sybil's web
product. Each scenario describes a user goal, fixture capabilities, permitted
side effects, visible assertions, evidence, cleanup, and reasons to stop. A
browser agent can change without rewriting the product contract.

These scenarios complement rather than replace Vitest and Playwright:

- Vitest owns deterministic component and state logic.
- Playwright owns repeatable browser protocol checks such as virtual WebAuthn,
  responsive geometry, and focus containment.
- Computer-use scenarios own exploratory, user-visible acceptance across a
  real assembled deployment.

Run the fast source gate or print the catalog from `frontend/web`:

```bash
pnpm scenarios:check
pnpm scenarios:list
node scripts/check-computer-use-scenarios.mjs --json
```

## Scenario contract

Scenario frontmatter has seven fields:

| Field          | Meaning                                                              |
| -------------- | -------------------------------------------------------------------- |
| `id`           | Stable kebab-case identity; it must equal the filename.              |
| `priority`     | `p0`, `p1`, or `p2`.                                                 |
| `mode`         | Maximum authority and side effects permitted by the run.             |
| `personas`     | User perspectives exercised.                                         |
| `routes`       | Product routes in scope; placeholders use `:name`.                   |
| `fixtures`     | Capabilities the environment must provide, never hard-coded row IDs. |
| `environments` | Required browser/device capabilities.                                |

Every file then uses the same ordered prose sections. The validator rejects
missing sections, duplicate IDs, unsafe route syntax, and browser-source
selectors. Scenarios should name controls by visible label, role, or user
meaning. They must not tell an agent how the React tree is implemented.

The side-effect modes are deliberately coarse:

- `read-only`: no server-side mutation. Local navigation, viewport changes,
  and connecting an already-provisioned account are allowed.
- `disposable-account`: mutations are limited to a fresh account provisioned
  for that run. Never borrow a human, bot, market-maker, or prior test account.
- `controlled-fault`: product state remains read-only, but an instrumented
  browser may selectively interrupt network reads or go offline.
- `operator`: privileged control-plane mutation. No executable operator
  scenario is present yet; using this mode requires explicit run-specific
  authorization.

## Runner protocol

Before execution, the runner must bind every named fixture and record only
non-secret identifiers. If a capability is missing, return `blocked`; do not
silently weaken the scenario or manufacture product data through an unrelated
admin path. Use a fresh browser profile unless the scenario says otherwise.

During execution:

1. Follow the numbered steps in order and interact through the rendered UI.
2. Observe every assertion independently. A later success does not erase an
   earlier false, misleading, inaccessible, or duplicated state.
3. Capture unexpected console errors and failed requests, but judge the result
   from the user's visible experience.
4. Never dismiss a warning, approve a mutation, reveal a credential, or change
   deployment state unless the scenario explicitly authorizes it.
5. Stop immediately on a listed stop condition. Preserve the evidence already
   collected and run safe cleanup where possible.

Store artifacts outside source control, for example under
`target/computer-use/<run-id>/`. A result record should have this shape:

```json
{
  "scenario_id": "passkey-order-lifecycle",
  "run_id": "2026-07-15T120000Z-chromium-mobile",
  "revision": "jj commit id or deployed image digest",
  "environment": "origin, browser, viewport, device/authenticator",
  "result": "pass | fail | blocked",
  "fixture_bindings": ["non-secret capability = opaque id"],
  "checkpoints": [
    { "step": 1, "result": "pass", "evidence": ["relative artifact path"] }
  ],
  "unexpected_console_errors": [],
  "failed_requests": [],
  "cleanup": "complete | partial | not-needed",
  "notes": "short user-visible finding"
}
```

Screenshots should include the whole relevant state and viewport dimensions.
Redact account keys, API keys, credential IDs, authorization headers, and
private user data. A screenshot alone is not proof of keyboard behavior,
recovery, or persistence; record the action and before/after observation.

## Adding scenarios

Prefer one coherent user goal over a tour of unrelated screens. Reuse a named
fixture capability only when its semantics really match. Keep protocol/API
details in architecture docs and deterministic tests; keep visible truth and
decision clarity here. If a required screen does not exist, open or reference a
product issue instead of writing an allegedly executable scenario.

After adding or changing a scenario, run `pnpm scenarios:check`, the focused
validator tests, and the ordinary frontend gate.
