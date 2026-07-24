import { defineConfig, devices } from "@playwright/test";

const baseURL =
  process.env.E2E_BASE_URL ?? "https://app.sybil.exchange";
const localHosts = (process.env.E2E_LOCAL_HOSTS ?? "")
  .split(",")
  .map((host) => host.trim())
  .filter(Boolean);

if (localHosts.length > 0 && !localHosts.includes(new URL(baseURL).hostname)) {
  throw new Error(
    `E2E_LOCAL_HOSTS must include the E2E_BASE_URL hostname (${new URL(baseURL).hostname})`,
  );
}

const localHostLaunchArgs = localHosts.length > 0
  ? [
      `--host-resolver-rules=${localHosts
        .map((host) => `MAP ${host} 127.0.0.1`)
        .join(",")}`,
    ]
  : [];

/**
 * Playwright config for the Sybil web e2e journey (SYB-243).
 *
 * Target: a LIVE deployment by default (there is no local backend the browser
 * journey can hit yet). Point at a compose stack later by overriding
 * `E2E_BASE_URL` (web app) and, if the API is not on the app host's parent
 * domain, `E2E_API_BASE`.
 *
 *   E2E_BASE_URL=https://app.sybil.exchange pnpm e2e
 *
 * A local HTTPS proxy can still exercise the validity-pinned devnet RP without
 * changing machine DNS or weakening verifier checks. Build the web/API with
 * that RP, point E2E_BASE_URL at the proxy, and set E2E_LOCAL_HOSTS to the
 * comma-separated proxy hostnames. Only Chromium maps them to 127.0.0.1, and
 * only this opt-in run accepts the ephemeral local CA.
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
    baseURL,
    ignoreHTTPSErrors: localHosts.length > 0,
    trace: "retain-on-failure",
    screenshot: "only-on-failure",
    video: "retain-on-failure",
  },
  projects: [
    {
      name: "chromium",
      use: {
        ...devices["Desktop Chrome"],
        ...(localHostLaunchArgs.length > 0
          ? { launchOptions: { args: localHostLaunchArgs } }
          : {}),
      },
    },
  ],
});
