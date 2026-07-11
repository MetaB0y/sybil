import {
  test,
  expect,
  type Page,
  type CDPSession,
  type APIRequestContext,
} from "@playwright/test";

/**
 * End-to-end passkey (WebAuthn) journey against a LIVE Sybil deployment.
 *
 * This is the test that would have caught the rp_id/origin blocker: a config
 * bug had the server's `webauthn_rp_id`/`origin` defaulting to localhost while
 * the browser registers/signs under the page origin (`app.<box>.nip.io`), so
 * every real passkey assertion was rejected server-side (RpIdHashMismatch /
 * OriginMismatch) even though the browser ceremony "succeeded".
 *
 * We drive a real Chromium WebAuthn *virtual authenticator* over CDP, create a
 * demo account with the DEFAULT passkey flow (not the raw dev-key toggle), then
 * place a signed order — which posts a genuine WebAuthn assertion to
 * `/v1/orders/signed`. If the server's rp_id/origin are misconfigured, the
 * order is rejected and this test fails loudly with the exact server error.
 *
 * Run:  E2E_BASE_URL=https://app.172-104-31-54.nip.io pnpm e2e
 */

const APP_BASE = process.env.E2E_BASE_URL ?? "https://app.172-104-31-54.nip.io";
const API_BASE = process.env.E2E_API_BASE ?? deriveApiBase(APP_BASE);

// Strings whose appearance in a console error or the UI means the passkey /
// signed-order path is broken (the exact failure class this test guards).
const CRITICAL =
  /OriginMismatch|RpIdHashMismatch|webauthn|passkey|signature|accepted=false|InvalidAssertion/i;

/** The API lives on the app host's PARENT domain: `app.<x>` → `<x>`. */
function deriveApiBase(appUrl: string): string {
  const u = new URL(appUrl);
  u.hostname = u.hostname.replace(/^app\./, "");
  return `${u.protocol}//${u.host}`;
}

interface MarketSummary {
  market_id: number;
  name: string;
  status?: string;
  yes_price_nanos: number | null;
}

interface Portfolio {
  balance_nanos: number | string;
  positions?: unknown[];
}

/**
 * Attach a Chromium virtual authenticator (ctap2 / internal transport, resident
 * key + user verification, presence auto-simulated) so `navigator.credentials`
 * resolves without a real device. Must be enabled before any WebAuthn ceremony.
 */
async function addVirtualAuthenticator(page: Page): Promise<CDPSession> {
  const client = await page.context().newCDPSession(page);
  await client.send("WebAuthn.enable");
  await client.send("WebAuthn.addVirtualAuthenticator", {
    options: {
      protocol: "ctap2",
      transport: "internal",
      hasResidentKey: true,
      hasUserVerification: true,
      isUserVerified: true,
      automaticPresenceSimulation: true,
    },
  });
  return client;
}

async function getJson<T>(
  request: APIRequestContext,
  url: string,
  readToken?: string,
): Promise<T> {
  const res = await request.get(url, {
    ...(readToken
      ? { headers: { authorization: `Bearer ${readToken}` } }
      : {}),
  });
  expect(res.ok(), `GET ${url} → ${res.status()}`).toBeTruthy();
  return (await res.json()) as T;
}

interface PricesResponse {
  prices: Record<string, { yes_price_nanos: number | null }>;
}

async function pickMarkets(
  request: APIRequestContext,
): Promise<{ priced?: MarketSummary; nullPrice?: MarketSummary }> {
  const [summary, pricesResp] = await Promise.all([
    getJson<MarketSummary[]>(request, `${API_BASE}/v1/markets/summary`),
    getJson<PricesResponse>(request, `${API_BASE}/v1/markets/prices`),
  ]);
  // The UI derives its indicative from the live `/v1/markets/prices` map (a
  // never-traded market is simply absent), so classify against that map — not
  // just the summary's snapshot field.
  const priceMap = pricesResp.prices ?? {};
  const priced = summary.find(
    (m) => m.status === "active" && priceMap[String(m.market_id)] != null,
  );
  const nullPrice = summary.find(
    (m) =>
      m.status === "active" &&
      m.yes_price_nanos == null &&
      priceMap[String(m.market_id)] == null,
  );
  return {
    ...(priced ? { priced } : {}),
    ...(nullPrice ? { nullPrice } : {}),
  };
}

test("passkey account create + signed order (live rp_id/origin validation)", async ({
  page,
  request,
}) => {
  const consoleErrors: string[] = [];
  const pageErrors: string[] = [];
  page.on("console", (m) => {
    if (m.type() === "error") consoleErrors.push(m.text());
  });
  page.on("pageerror", (e) => pageErrors.push(e.message));

  await addVirtualAuthenticator(page);

  // 1. Land.
  await page.goto("/");
  const connect = page.getByRole("button", { name: "connect", exact: true });
  await expect(connect).toBeVisible();
  await connect.click();

  // 2. Create a demo account with the DEFAULT passkey flow. When WebAuthn is
  //    available the "Create demo" tab defaults its key mode to Passkey, so the
  //    primary button reads "Create with passkey" — clicking it runs a real
  //    registration ceremony through the virtual authenticator.
  const connectDialog = page.getByRole("dialog", { name: "Connect" });
  await expect(connectDialog).toBeVisible();
  const createBtn = connectDialog.getByRole("button", {
    name: "Create with passkey",
  });
  await expect(
    createBtn,
    "default create mode should be Passkey (WebAuthn available)",
  ).toBeVisible();
  await createBtn.click();

  // 3. On success the modal closes and the nav chip flips to connected. The
  //    connected chip is the menu button whose title recaps the portfolio
  //    (distinguishes it from the "Dev Zone" nav menu, which shares the role).
  await expect(connectDialog).toBeHidden({ timeout: 30_000 });
  const accountMenu = page.locator(
    'button[aria-haspopup="menu"][title*="Portfolio"]',
  );
  await expect(
    accountMenu,
    "passkey registration should connect the account",
  ).toBeVisible({ timeout: 30_000 });

  // The persisted account must be a webauthn (passkey) account, not raw_p256.
  const authScheme = await page.evaluate(() =>
    localStorage.getItem("sybil:auth:auth_scheme"),
  );
  expect(authScheme, "account should be a passkey (webauthn) account").toBe(
    "webauthn",
  );
  const accountIdRaw = await page.evaluate(() =>
    localStorage.getItem("sybil:auth:account_id"),
  );
  expect(accountIdRaw, "account id should be persisted").toBeTruthy();
  const accountId = Number(accountIdRaw);

  // 4. Disconnect. This deliberately clears every sybil:auth localStorage key
  //    while leaving the resident credential in the virtual authenticator.
  await accountMenu.click();
  const accountDropdown = page.getByRole("menu");
  await accountDropdown.getByRole("menuitem", { name: "Disconnect" }).click();
  await expect(
    connect,
    "disconnect should return to the connect state",
  ).toBeVisible();
  expect(
    await page.evaluate(() => localStorage.getItem("sybil:auth:account_id")),
    "disconnect should clear the locally saved account",
  ).toBeNull();
  expect(
    await page.evaluate(() => localStorage.getItem("sybil:auth:read_api_key")),
    "disconnect should clear the read API key",
  ).toBeNull();

  // 5. Reconnect through an empty allowCredentials list. Chromium's resident
  //    credential returns the original 8-byte userHandle, so the app can
  //    recover the account id and registered public key without local state.
  await connect.click();
  await expect(connectDialog).toBeVisible();
  await connectDialog
    .getByRole("button", { name: "Passkey", exact: true })
    .first()
    .click();
  const signInButton = connectDialog.getByRole("button", {
    name: "Sign in with passkey",
  });
  await expect(signInButton).toBeEnabled();
  await signInButton.click();

  await expect(connectDialog).toBeHidden({ timeout: 30_000 });
  await expect(
    accountMenu,
    "discoverable passkey sign-in should reconnect the account",
  ).toBeVisible({ timeout: 30_000 });
  const recoveredAccountIdRaw = await page.evaluate(() =>
    localStorage.getItem("sybil:auth:account_id"),
  );
  expect(
    Number(recoveredAccountIdRaw),
    "discoverable sign-in should recover the same account",
  ).toBe(accountId);
  const readToken = await page.evaluate(() =>
    localStorage.getItem("sybil:auth:read_api_key"),
  );
  expect(readToken, "passkey login should mint and persist a read key").toMatch(
    /^sybk_/,
  );

  // 6. Confirm the reconnected account has its demo balance (capped $5k).
  const pf0 = await getJson<Portfolio>(
    request,
    `${API_BASE}/v1/accounts/${accountId}/portfolio`,
    readToken!,
  );
  const balance0 = BigInt(pf0.balance_nanos);
  expect(
    balance0,
    "new passkey account should be funded with demo balance",
  ).toBeGreaterThan(0n);

  // 7. Open a market that has a price.
  const { priced } = await pickMarkets(request);
  expect(priced, "need an active market with a price").toBeTruthy();
  const market = priced!;
  await page.goto(`/m/${market.market_id}`);

  const placeOrder = page.getByRole("button", { name: "Place order" });
  await expect(placeOrder.first()).toBeVisible({ timeout: 20_000 });
  await placeOrder.first().click();

  const orderDialog = page.getByRole("dialog", { name: "Place order" });
  await expect(orderDialog).toBeVisible();

  // A priced market must present a real indicative estimate (not the null-price
  // seed-the-book copy) — sanity that we picked a live market.
  await expect(orderDialog).toContainText(/est\. fill · next batch/i);

  // 8. Submit a signed BUY YES (default BuyBox state: buy / YES / $25 / GTC).
  //    Clicking the CTA runs a WebAuthn assertion → POST /v1/orders/signed.
  const cta = orderDialog.getByRole("button", { name: /review buy|queue buy/i });
  await expect(cta).toBeVisible();
  await cta.click();
  const confirm = orderDialog.getByRole("button", { name: /confirm buy/i });
  if (await confirm.isVisible().catch(() => false)) await confirm.click();

  // 9. Assert the signed order was ACCEPTED — and fail loudly, with the exact
  //    server error, if the passkey assertion was rejected (the rp_id/origin
  //    regression class). Race the accepted receipt against the error alert.
  const acceptedStatus = orderDialog
    .getByRole("status")
    .filter({ hasText: /order accepted/i });
  const errorAlert = orderDialog.locator('[role="alert"]');

  const outcome = await Promise.race([
    acceptedStatus
      .waitFor({ state: "visible", timeout: 30_000 })
      .then(() => "accepted" as const),
    errorAlert
      .first()
      .waitFor({ state: "visible", timeout: 30_000 })
      .then(() => "rejected" as const),
  ]).catch(() => "timeout" as const);

  if (outcome !== "accepted") {
    let detail = "(no error text surfaced)";
    if ((await errorAlert.count()) > 0) {
      detail = await errorAlert
        .first()
        .innerText()
        .catch(() => detail);
    }
    await page.screenshot({
      path: `test-results/signed-order-failure-${Date.now()}.png`,
      fullPage: true,
    });
    throw new Error(
      `Signed order did NOT succeed (${outcome}). UI error: ${detail}`,
    );
  }
  await expect(acceptedStatus).toBeVisible();

  // The UI must never surface a WebAuthn origin / rp-id mismatch.
  const bodyText = await page.locator("body").innerText();
  expect(bodyText, "UI surfaced a WebAuthn origin/rp-id mismatch").not.toMatch(
    /OriginMismatch|RpIdHashMismatch/i,
  );

  // No console error / uncaught exception referencing the passkey/order path.
  const criticalConsole = consoleErrors.filter((e) => CRITICAL.test(e));
  expect(
    criticalConsole,
    `console errors on the passkey/order path:\n${criticalConsole.join("\n")}`,
  ).toEqual([]);
  expect(pageErrors, `uncaught page errors:\n${pageErrors.join("\n")}`).toEqual(
    [],
  );

  // 10. Within ~2 blocks (10s each), the order must leave a trace: a pending
  //    order, a fill, a position, or a reserved-balance decrease. Any of these
  //    proves the signature verified server-side.
  await expect(async () => {
    const [orders, fills, pf] = await Promise.all([
      getJson<unknown[]>(
        request,
        `${API_BASE}/v1/accounts/${accountId}/orders`,
        readToken!,
      ),
      getJson<unknown[]>(
        request,
        `${API_BASE}/v1/accounts/${accountId}/fills?limit=10`,
        readToken!,
      ),
      getJson<Portfolio>(
        request,
        `${API_BASE}/v1/accounts/${accountId}/portfolio`,
        readToken!,
      ),
    ]);
    const hasPending = Array.isArray(orders) && orders.length > 0;
    const hasFill = Array.isArray(fills) && fills.length > 0;
    const hasPosition = Array.isArray(pf.positions) && pf.positions.length > 0;
    const balanceDropped = BigInt(pf.balance_nanos) < balance0;
    expect(
      hasPending || hasFill || hasPosition || balanceDropped,
      "signed order left no trace (no pending / fill / position / balance change) — signature likely failed verify",
    ).toBeTruthy();
  }).toPass({ timeout: 30_000, intervals: [1_000, 2_000, 3_000] });
});

test("never-traded market does not fabricate a 50c quote", async ({
  page,
  request,
}) => {
  const { nullPrice } = await pickMarkets(request);
  test.skip(
    !nullPrice,
    "no active never-traded (null-price) market on this deployment",
  );
  const market = nullPrice!;

  await page.goto(`/m/${market.market_id}`);
  const placeOrder = page.getByRole("button", { name: "Place order" });
  await expect(placeOrder.first()).toBeVisible({ timeout: 20_000 });
  await placeOrder.first().click();

  const orderDialog = page.getByRole("dialog", { name: "Place order" });
  await expect(orderDialog).toBeVisible();

  // The UX fix: a never-traded market says "no price yet / seed the book" and
  // offers "no indicative yet" — it must NOT present a fabricated ~50% estimate.
  await expect(orderDialog).toContainText(/no price yet/i);
  await expect(orderDialog).toContainText(/seed the book/i);
  await expect(orderDialog).toContainText(/no indicative yet/i);
  await expect(
    orderDialog.getByText(/est\. (fill|proceeds) · next batch/i),
    "null-price market must not show a fabricated clearing estimate",
  ).toHaveCount(0);
});
