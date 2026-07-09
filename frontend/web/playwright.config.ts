import { defineConfig, devices } from "@playwright/test";

/**
 * Playwright config for the Sybil web e2e journey (SYB-243).
 *
 * Target: a LIVE deployment by default (there is no local backend the browser
 * journey can hit yet). Point at a compose stack later by overriding
 * `E2E_BASE_URL` (web app) and, if the API is not on the app host's parent
 * domain, `E2E_API_BASE`.
 *
 *   E2E_BASE_URL=https://app.172-104-31-54.nip.io pnpm e2e
 *
 * The spec drives the passkey (WebAuthn) account-create + signed-order flow via
 * a Chromium virtual authenticator (CDP), so we only run the Chromium project.
 */
export default defineConfig({
  testDir: "./e2e",
  // Passkey ceremonies + batch settling (~10s/block) make this a slow, live
  // journey; give each test room and don't parallelise against one backend.
  timeout: 120_000,
  expect: { timeout: 15_000 },
  fullyParallel: false,
  workers: 1,
  forbidOnly: !!process.env.CI,
  retries: process.env.CI ? 1 : 0,
  reporter: process.env.CI
    ? [["github"], ["list"], ["html", { open: "never" }]]
    : [["list"], ["html", { open: "never" }]],
  use: {
    baseURL: process.env.E2E_BASE_URL ?? "https://app.172-104-31-54.nip.io",
    trace: "retain-on-failure",
    screenshot: "only-on-failure",
    video: "retain-on-failure",
  },
  projects: [
    {
      name: "chromium",
      use: { ...devices["Desktop Chrome"] },
    },
  ],
});
