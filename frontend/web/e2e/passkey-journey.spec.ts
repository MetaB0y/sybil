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
// Chromium's opt-in host-resolver rules do not apply to Playwright's
// APIRequestContext. Local pinned-RP runs can therefore keep browser traffic on
// the HTTPS proxy while sending read-after-write assertions straight to the
// loopback API.
const REQUEST_API_BASE = process.env.E2E_REQUEST_API_BASE ?? API_BASE;

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

interface PendingOrder {
  order_id: number;
  market_id: number;
  limit_price_nanos: number | string;
}

interface SigningKeyResponse {
  public_key_hex: string;
  label?: string | null;
}

function normalizePublicKeyHex(value: string): string {
  return value.replace(/^0x/i, "").toLowerCase();
}

/**
 * Attach a Chromium virtual authenticator (ctap2 / internal transport, resident
 * key + user verification, presence auto-simulated) so `navigator.credentials`
 * resolves without a real device. Must be enabled before any WebAuthn ceremony.
 */
interface VirtualAuthenticator {
  client: CDPSession;
  authenticatorId: string;
}

type VirtualAuthenticatorTransport = "usb" | "ble" | "nfc" | "internal";

async function addVirtualAuthenticator(
  client: CDPSession,
  transport: VirtualAuthenticatorTransport,
  automaticPresenceSimulation: boolean,
): Promise<string> {
  const { authenticatorId } = await client.send(
    "WebAuthn.addVirtualAuthenticator",
    {
      options: {
        protocol: "ctap2",
        transport,
        hasResidentKey: true,
        hasUserVerification: true,
        isUserVerified: true,
        automaticPresenceSimulation,
      },
    },
  );
  return authenticatorId;
}

async function enableVirtualAuthenticator(
  page: Page,
): Promise<VirtualAuthenticator> {
  const client = await page.context().newCDPSession(page);
  await client.send("WebAuthn.enable");
  const authenticatorId = await addVirtualAuthenticator(
    client,
    "internal",
    true,
  );
  return { client, authenticatorId };
}

async function getJson<T>(
  request: APIRequestContext,
  url: string,
  readToken?: string,
): Promise<T> {
  const res = await request.get(url, {
    ...(readToken ? { headers: { authorization: `Bearer ${readToken}` } } : {}),
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
    getJson<MarketSummary[]>(request, `${REQUEST_API_BASE}/v1/markets/summary`),
    getJson<PricesResponse>(request, `${REQUEST_API_BASE}/v1/markets/prices`),
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

  const authenticator = await enableVirtualAuthenticator(page);
  let injectedStaleBootstrapBinding = false;
  await page.route(
    "**/v1/accounts/*/keys/revoke",
    async (route) => {
      injectedStaleBootstrapBinding = true;
      await route.fulfill({
        status: 409,
        contentType: "application/json",
        body: JSON.stringify({
          error: "stale key-operation state binding for onboarding account",
          code: "CONFLICT",
        }),
      });
    },
    { times: 1 },
  );

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

  // 3. On success the modal closes and the nav chip flips to the labeled
  //    account menu (distinguishes it from the "Dev Zone" nav menu).
  await expect(connectDialog).toBeHidden({ timeout: 30_000 });
  expect(
    injectedStaleBootstrapBinding,
    "onboarding should retry the intentionally stale bootstrap revoke",
  ).toBe(true);
  const accountMenu = page.getByRole("button", { name: /account menu/i });
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
  const originalPublicKeyHex = await page.evaluate(() =>
    localStorage.getItem("sybil:auth:pubkey_hex"),
  );
  expect(
    originalPublicKeyHex,
    "primary passkey public key should be persisted",
  ).toMatch(/^(02|03)[0-9a-f]{64}$/i);

  // 4. Register a second passkey from Settings using a distinct virtual
  //    authenticator, modeling another device/provider. The backup device must
  //    handle credential creation; the primary must then return to authorize
  //    the state-bound key registration.
  const primaryCredentials = await authenticator.client.send(
    "WebAuthn.getCredentials",
    { authenticatorId: authenticator.authenticatorId },
  );
  expect(primaryCredentials.credentials).toHaveLength(1);

  await authenticator.client.send("WebAuthn.setAutomaticPresenceSimulation", {
    authenticatorId: authenticator.authenticatorId,
    enabled: false,
  });
  const backupAuthenticatorId = await addVirtualAuthenticator(
    authenticator.client,
    "usb",
    true,
  );
  const backupCredentialReady = new Promise<void>((resolve, reject) => {
    const onCredentialAdded = (event: { authenticatorId: string }) => {
      if (event.authenticatorId !== backupAuthenticatorId) return;
      authenticator.client.off("WebAuthn.credentialAdded", onCredentialAdded);
      void (async () => {
        await authenticator.client.send(
          "WebAuthn.setAutomaticPresenceSimulation",
          { authenticatorId: backupAuthenticatorId, enabled: false },
        );
        await authenticator.client.send(
          "WebAuthn.setAutomaticPresenceSimulation",
          { authenticatorId: authenticator.authenticatorId, enabled: true },
        );
        resolve();
      })().catch(reject);
    };
    authenticator.client.on("WebAuthn.credentialAdded", onCredentialAdded);
  });

  // Hold the key-operation binding request until the CDP event has switched
  // simulated presence back to the primary authenticator. This removes a race
  // between navigator.credentials.create() resolving and the authorization
  // assertion that follows it.
  await page.route(
    `**/v1/accounts/${accountId}/keyop-state`,
    async (route) => {
      await backupCredentialReady;
      await route.continue();
    },
    { times: 1 },
  );

  await page.goto("/settings");
  const addBackup = page.getByRole("button", { name: "Add backup passkey" });
  await expect(addBackup).toBeEnabled();
  await addBackup.click();
  await expect(page.getByRole("status")).toContainText("Backup passkey added");
  await expect(page.getByText(/backup passkey · webauthn/i)).toBeVisible();

  // Show-once credentials must never claim a clipboard write succeeded when
  // the browser denies it. Exercise the real signed API-key creation, then
  // verify the modal selects the secret for manual copying and restores focus.
  const apiKeyLabel = page.getByPlaceholder("e.g. grafana");
  const createApiKey = page.getByRole("button", { name: "Create API key" });
  await apiKeyLabel.fill("e2e show-once");
  await createApiKey.click();
  const showOnce = page.getByRole("dialog", {
    name: "Read API key created",
  });
  await expect(showOnce).toBeVisible({ timeout: 30_000 });
  await expect(showOnce.getByRole("button", { name: "Close" })).toBeFocused();
  await expect(page.locator("body")).toHaveCSS("overflow", "hidden");
  await page.evaluate(() => {
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: {
        writeText: async () => {
          throw new DOMException("clipboard denied", "NotAllowedError");
        },
      },
    });
  });
  await showOnce.getByRole("button", { name: "Copy", exact: true }).click();
  await expect(showOnce.getByRole("alert")).toContainText("Copy failed");
  await expect(
    showOnce.getByRole("button", { name: "Try copy again" }),
  ).toBeVisible();
  const secretField = showOnce.getByRole("textbox", { name: "Bearer token" });
  await expect(secretField).toBeFocused();
  expect(
    await secretField.evaluate(
      (field) =>
        field instanceof HTMLTextAreaElement &&
        field.selectionStart === 0 &&
        field.selectionEnd === field.value.length,
    ),
    "failed clipboard writes should select the entire one-time secret",
  ).toBe(true);
  await page.evaluate(() => {
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: { writeText: async () => undefined },
    });
  });
  const retryCopy = showOnce.getByRole("button", { name: "Try copy again" });
  await retryCopy.click();
  const copied = showOnce.getByRole("button", { name: "Copied ✓" });
  await expect(copied).toBeVisible();
  await expect(copied).toBeFocused();
  await page.keyboard.press("Escape");
  await expect(showOnce).toBeHidden();
  await expect(page.locator("body")).not.toHaveCSS("overflow", "hidden");
  await expect(createApiKey).toBeFocused();

  const backupCredentials = await authenticator.client.send(
    "WebAuthn.getCredentials",
    { authenticatorId: backupAuthenticatorId },
  );
  expect(backupCredentials.credentials).toHaveLength(1);
  await authenticator.client.send("WebAuthn.setAutomaticPresenceSimulation", {
    authenticatorId: backupAuthenticatorId,
    enabled: true,
  });
  await authenticator.client.send("WebAuthn.removeVirtualAuthenticator", {
    authenticatorId: authenticator.authenticatorId,
  });

  // 5. Disconnect. This deliberately clears the persistent identity and the
  //    session-scoped read token while leaving only the backup authenticator.
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
    await page.evaluate(() => sessionStorage.getItem("sybil:auth:read_api_key")),
    "disconnect should clear the read API key",
  ).toBeNull();

  // 6. Reconnect through an empty allowCredentials list. Chromium's resident
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
    "backup passkey sign-in should reconnect the account",
  ).toBeVisible({ timeout: 30_000 });
  const recoveredAccountIdRaw = await page.evaluate(() =>
    localStorage.getItem("sybil:auth:account_id"),
  );
  expect(
    Number(recoveredAccountIdRaw),
    "discoverable sign-in should recover the same account",
  ).toBe(accountId);
  const readToken = await page.evaluate(() =>
    sessionStorage.getItem("sybil:auth:read_api_key"),
  );
  expect(
    readToken,
    "passkey login should mint a session-scoped read key",
  ).toMatch(/^sybk_/);
  expect(
    await page.evaluate(() => localStorage.getItem("sybil:auth:read_api_key")),
    "bearer read keys must not cross the persistent-storage boundary",
  ).toBeNull();

  // 7. Complete recovery from the backup session: the connected backup key is
  //    deliberately not revocable, while the original browser passkey is.
  //    Revoking the original must be signed by the backup and leave exactly the
  //    tested backup credential active before we rely on it for trading.
  const recoveredPublicKeyHex = await page.evaluate(() =>
    localStorage.getItem("sybil:auth:pubkey_hex"),
  );
  expect(
    recoveredPublicKeyHex,
    "backup passkey public key should be persisted",
  ).toMatch(/^(02|03)[0-9a-f]{64}$/i);
  expect(normalizePublicKeyHex(recoveredPublicKeyHex!)).not.toBe(
    normalizePublicKeyHex(originalPublicKeyHex!),
  );

  await page.goto("/settings");
  const backupKeyRow = page.locator(".settings-row", {
    hasText: "backup passkey · webauthn",
  });
  const originalKeyRow = page.locator(".settings-row", {
    hasText: "browser passkey · webauthn",
  });
  await expect(backupKeyRow).toBeVisible({ timeout: 20_000 });
  await expect(originalKeyRow).toBeVisible({ timeout: 20_000 });
  await expect(
    backupKeyRow.getByRole("button", { name: "Revoke" }),
  ).toBeDisabled();
  await expect(
    backupKeyRow.getByRole("button", { name: "Revoke" }),
  ).toHaveAttribute(
    "title",
    "Reconnect with another registered key before revoking this one",
  );

  const revokeResponsePromise = page.waitForResponse(
    (response) =>
      response.url().endsWith(`/v1/accounts/${accountId}/keys/revoke`) &&
      response.request().method() === "POST",
  );
  await originalKeyRow.getByRole("button", { name: "Revoke" }).click();
  const revokeResponse = await revokeResponsePromise;
  expect(
    revokeResponse.ok(),
    `backup-signed key revocation should succeed: HTTP ${revokeResponse.status()}`,
  ).toBeTruthy();
  await expect(originalKeyRow).toHaveCount(0, { timeout: 10_000 });
  await expect(backupKeyRow).toBeVisible();

  const remainingKeys = await getJson<SigningKeyResponse[]>(
    request,
    `${REQUEST_API_BASE}/v1/accounts/${accountId}/keys`,
    readToken!,
  );
  expect(remainingKeys).toHaveLength(1);
  expect(remainingKeys[0]?.label).toBe("backup passkey");
  expect(normalizePublicKeyHex(remainingKeys[0]!.public_key_hex)).toBe(
    normalizePublicKeyHex(recoveredPublicKeyHex!),
  );

  // 8. Confirm the reconnected account has the server-assigned demo grant.
  const pf0 = await getJson<Portfolio>(
    request,
    `${REQUEST_API_BASE}/v1/accounts/${accountId}/portfolio`,
    readToken!,
  );
  const balance0 = BigInt(pf0.balance_nanos);
  expect(
    balance0,
    "new passkey account should be funded with demo balance",
  ).toBeGreaterThan(0n);

  // 9. Open a market that has a price.
  const { priced } = await pickMarkets(request);
  expect(priced, "need an active market with a price").toBeTruthy();
  const market = priced!;
  // The compact layout opens the reviewed order sheet; desktop Lite exposes
  // direct bet controls instead of a separate "Place order" button.
  await page.setViewportSize({ width: 390, height: 844 });
  await page.goto(`/m/${market.market_id}`);

  const placeOrder = page.getByRole("button", { name: "Place order" });
  await expect(placeOrder.first()).toBeVisible({ timeout: 20_000 });
  await placeOrder.first().click();

  const orderDialog = page.getByRole("dialog", { name: "Place order" });
  await expect(orderDialog).toBeVisible();

  // A priced market must present its real indicative anchor (not the
  // null-price seed-the-book copy) — sanity that we picked a live market.
  await expect(orderDialog).toContainText(/indicative \d+¢/i);
  await expect(orderDialog).not.toContainText(
    /no indicative yet|seed the book/i,
  );

  // 10. Submit a signed BUY YES (default BuyBox state: buy / YES / $25 / GTC).
  //    Clicking the CTA runs a WebAuthn assertion → POST /v1/orders/signed.
  const cta = orderDialog.getByRole("button", {
    name: /review buy|queue buy/i,
  });
  await expect(cta).toBeVisible();
  await cta.click();
  const confirm = orderDialog.getByRole("button", { name: /confirm buy/i });
  if (await confirm.isVisible().catch(() => false)) await confirm.click();

  // 11. Assert the signed order was ACCEPTED — and fail loudly, with the exact
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

  // 12. Within ~2 blocks (10s each), the order must leave a trace: a pending
  //    order, a fill, a position, or a reserved-balance decrease. Any of these
  //    proves the signature verified server-side.
  await expect(async () => {
    const [orders, fills, pf] = await Promise.all([
      getJson<{ fills: unknown[] }>(
        request,
        `${REQUEST_API_BASE}/v1/accounts/${accountId}/orders`,
        readToken!,
      ),
      getJson<{ fills: unknown[] }>(
        request,
        `${REQUEST_API_BASE}/v1/accounts/${accountId}/fills?limit=10`,
        readToken!,
      ),
      getJson<Portfolio>(
        request,
        `${REQUEST_API_BASE}/v1/accounts/${accountId}/portfolio`,
        readToken!,
      ),
    ]);
    const hasPending = Array.isArray(orders) && orders.length > 0;
    const hasFill = Array.isArray(fills.fills) && fills.fills.length > 0;
    const hasPosition = Array.isArray(pf.positions) && pf.positions.length > 0;
    const balanceDropped = BigInt(pf.balance_nanos) < balance0;
    expect(
      hasPending || hasFill || hasPosition || balanceDropped,
      "signed order left no trace (no pending / fill / position / balance change) — signature likely failed verify",
    ).toBeTruthy();
  }).toPass({ timeout: 30_000, intervals: [1_000, 2_000, 3_000] });

  // 13. Create a 1c GTC order immediately after a fresh batch, then cancel it
  //     before the next batch. This gives the UI an authoritative resting order
  //     id without assuming a busy deployment still has a never-traded market.
  const cancelMarket = market;
  await page.goto(`/m/${cancelMarket.market_id}`);
  const cancelMarketPlaceOrder = page
    .getByRole("button", { name: "Place order" })
    .first();
  await expect(cancelMarketPlaceOrder).toBeVisible({ timeout: 20_000 });
  await cancelMarketPlaceOrder.click();

  const cancelOrderDialog = page.getByRole("dialog", { name: "Place order" });
  const limitSlider = cancelOrderDialog.getByRole("slider");
  await limitSlider.focus();
  await limitSlider.press("Home");
  await expect(limitSlider).toHaveValue("1");

  const heightBefore = (
    await getJson<{ height: number }>(
      request,
      `${REQUEST_API_BASE}/v1/blocks/latest`,
    )
  ).height;
  await expect
    .poll(
      async () =>
        (
          await getJson<{ height: number }>(
            request,
            `${REQUEST_API_BASE}/v1/blocks/latest`,
          )
        ).height,
      {
        message: "need a fresh batch boundary before cancel coverage",
        timeout: 20_000,
        intervals: [500, 1_000],
      },
    )
    .toBeGreaterThan(heightBefore);

  const queueCancelCandidate = cancelOrderDialog.getByRole("button", {
    name: /review buy|queue buy/i,
  });
  await queueCancelCandidate.click();
  const confirmCancelCandidate = cancelOrderDialog.getByRole("button", {
    name: /confirm buy/i,
  });
  if (await confirmCancelCandidate.isVisible().catch(() => false)) {
    await confirmCancelCandidate.click();
  }
  await expect(
    cancelOrderDialog
      .getByRole("status")
      .filter({ hasText: /order accepted/i }),
  ).toBeVisible({ timeout: 30_000 });

  let cancelOrderId: number | undefined;
  await expect(async () => {
    const orders = await getJson<PendingOrder[]>(
      request,
      `${REQUEST_API_BASE}/v1/accounts/${accountId}/orders`,
      readToken!,
    );
    const resting = orders.find(
      (order) =>
        order.market_id === cancelMarket.market_id &&
        BigInt(order.limit_price_nanos) === 10_000_000n,
    );
    expect(
      resting,
      "1c GTC order should be resting before cancellation",
    ).toBeTruthy();
    cancelOrderId = resting!.order_id;
  }).toPass({ timeout: 30_000, intervals: [500, 1_000, 2_000] });
  expect(cancelOrderId).toBeDefined();

  // 14. Cancel through the real portfolio UI. The remaining backup passkey
  //     signs `/v1/orders/cancel/signed`; the row should disappear immediately
  //     from the shared cache, then the API must confirm it is no longer open.
  await page.goto("/portfolio");
  const bridgeKey = await getJson<{ sybil_account_key_hex: string }>(
    request,
    `${REQUEST_API_BASE}/v1/accounts/${accountId}/bridge-key`,
    readToken!,
  );
  const displayedBridgeKey = bridgeKey.sybil_account_key_hex.startsWith("0x")
    ? bridgeKey.sybil_account_key_hex
    : `0x${bridgeKey.sybil_account_key_hex}`;
  const depositGuide = page.getByRole("region", { name: "L1 deposits" });
  await expect(depositGuide).toContainText(displayedBridgeKey, {
    timeout: 20_000,
  });
  await expect(depositGuide).toContainText(/no L1 refund flow today/i);
  const withdrawals = page.getByRole("region", { name: "Normal withdrawals" });
  await expect(withdrawals).toContainText("0 active", { timeout: 20_000 });
  await expect(withdrawals).toContainText("No active withdrawal leaves");
  await expect(withdrawals).toContainText(
    /new withdrawal requests are not enabled/i,
  );
  await expect(withdrawals).toContainText(
    /relay, delayed L1 finalization, and confirmed-log indexing are separate steps/i,
  );
  await expect(withdrawals).toContainText(
    /accept-all mock relay is not real-funds proof security/i,
  );
  await page.getByRole("tab", { name: /open orders/i }).click();
  const cancelRow = page.locator(`[data-order-id="${cancelOrderId}"]`);
  await expect(cancelRow).toBeVisible({ timeout: 20_000 });
  const cancelResponsePromise = page.waitForResponse(
    (response) =>
      response.url().endsWith("/v1/orders/cancel/signed") &&
      response.request().method() === "POST",
  );
  await cancelRow.getByRole("button", { name: "Cancel" }).click();
  const cancelResponse = await cancelResponsePromise;
  expect(
    cancelResponse.ok(),
    `signed cancel should succeed: HTTP ${cancelResponse.status()}`,
  ).toBeTruthy();
  expect(await cancelResponse.json()).toMatchObject({ cancelled: true });
  await expect(cancelRow).toHaveCount(0, { timeout: 10_000 });

  await expect(async () => {
    const orders = await getJson<PendingOrder[]>(
      request,
      `${REQUEST_API_BASE}/v1/accounts/${accountId}/orders`,
      readToken!,
    );
    expect(
      orders.some((order) => order.order_id === cancelOrderId),
      "cancelled order must be absent from authoritative open-order state",
    ).toBe(false);
  }).toPass({ timeout: 20_000, intervals: [500, 1_000, 2_000] });

  // The full create → recover → order → cancel path must never surface a
  // WebAuthn origin/rp-id mismatch or an uncaught browser error.
  const bodyText = await page.locator("body").innerText();
  expect(bodyText, "UI surfaced a WebAuthn origin/rp-id mismatch").not.toMatch(
    /OriginMismatch|RpIdHashMismatch/i,
  );
  const criticalConsole = consoleErrors.filter((e) => CRITICAL.test(e));
  expect(
    criticalConsole,
    `console errors on the passkey/order path:\n${criticalConsole.join("\n")}`,
  ).toEqual([]);
  expect(pageErrors, `uncaught page errors:\n${pageErrors.join("\n")}`).toEqual(
    [],
  );
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

  // The Place order sheet is the compact-layout entry point; desktop keeps
  // order entry in the reviewed right-hand Lite/Pro rail instead.
  await page.setViewportSize({ width: 390, height: 844 });
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
