import { expect, test, type Locator, type Page } from "@playwright/test";

const MOBILE_VIEWPORT = { width: 390, height: 844 };
const APP_BASE = process.env.E2E_BASE_URL ?? "https://app.172-104-31-54.nip.io";
const API_BASE = process.env.E2E_API_BASE ?? "https://172-104-31-54.nip.io";

test.describe("mobile viewport smoke", () => {
  test.use({ viewport: MOBILE_VIEWPORT, isMobile: true, hasTouch: true });
  test.afterEach(async ({ page }) => {
    await page.unrouteAll({ behavior: "ignoreErrors" });
  });

  test("core public pages fit and market order entry is reachable", async ({
    page,
  }) => {
    await proxyApiForLocalRun(page);
    await page.goto("/");

    const navMenu = page.getByRole("button", {
      name: "Open navigation menu",
    });
    await expect(navMenu).toBeVisible();
    await expect(page.locator(".global-nav-tabs")).toBeHidden();

    const marketsGrid = page.getByTestId("markets-grid");
    await expect(marketsGrid).toBeVisible({ timeout: 30_000 });
    await expect
      .poll(() => gridColumnCount(marketsGrid), {
        message: "the 390px markets grid should have exactly one column",
      })
      .toBe(1);
    await expectNoDocumentOverflow(page, "/");
    await expectTouchTargets(page, "/");

    const firstMarket = marketsGrid.locator('a[href^="/m/"]').first();
    await expect(firstMarket).toBeVisible();
    const href = await firstMarket.getAttribute("href");
    expect(href).toMatch(/^\/m\/\d+/);
    await page.goto(href!);

    await expect(page.getByTestId("market-detail-grid")).toBeVisible({
      timeout: 30_000,
    });
    const chart = page.getByTestId("price-chart-interaction");
    await expect(chart).toBeVisible();
    await chart.scrollIntoViewIfNeeded();
    const chartBox = await chart.boundingBox();
    expect(chartBox).not.toBeNull();
    await chart.tap({
      position: {
        x: chartBox!.width * 0.95,
        y: chartBox!.height * 0.5,
      },
    });
    await expect(page.getByTestId("price-chart-tooltip")).toBeVisible();
    await expect(chart).toHaveCSS("touch-action", "pan-y");

    const placeOrder = page.getByRole("button", { name: "Place order" });
    await expect(placeOrder).toBeVisible();
    await placeOrder.click();

    const dialog = page.getByRole("dialog", { name: "Place order" });
    await expect(dialog).toBeVisible();
    const closeOrder = dialog.getByRole("button", { name: "Close" });
    await expect(closeOrder).toBeFocused();
    const sheet = dialog.locator(".place-order-sheet");
    await expect(sheet).toBeVisible();
    const bottomGap = await sheet.evaluate(
      (node) => window.innerHeight - node.getBoundingClientRect().bottom,
    );
    expect(Math.abs(bottomGap)).toBeLessThanOrEqual(1);
    await expectNoDocumentOverflow(page, href!);
    await expectTouchTargets(page, `${href!} order sheet`);

    await closeOrder.click();
    await expect(dialog).toBeHidden();
    await expect(placeOrder).toBeFocused();

    for (const path of ["/portfolio", "/activity", "/arena"]) {
      await page.goto(path);
      await expect(page.locator("main")).toBeVisible();
      await expectNoDocumentOverflow(page, path);
      await expectTouchTargets(page, path);
    }
  });

  test("connect dialog owns focus and restores the page on close", async ({
    page,
  }) => {
    await proxyApiForLocalRun(page);
    await page.goto("/");

    const connect = page.getByRole("button", { name: "connect", exact: true });
    await expect(connect).toBeVisible();
    await connect.focus();
    await connect.click();

    const dialog = page.getByRole("dialog", { name: "Connect" });
    await expect(dialog).toBeVisible();
    const close = dialog.getByRole("button", { name: "Close" });
    await expect(close).toBeFocused();
    await expect
      .poll(() => page.evaluate(() => document.body.style.overflow))
      .toBe("hidden");

    await page.keyboard.press("Escape");
    await expect(dialog).toBeHidden();
    await expect(connect).toBeFocused();
    await expect
      .poll(() => page.evaluate(() => document.body.style.overflow))
      .toBe("");
  });

  test("deployment fixtures stay out of index and global search", async ({
    page,
  }) => {
    await proxyApiForLocalRun(page);
    await page.goto("/?q=SYB-247&closed=show");

    await expect(page.getByText("no events match these filters.")).toBeVisible({
      timeout: 30_000,
    });
    await expect(
      page.locator('a[href^="/m/"]').filter({
        hasText: /^SYB-247 deterministic crossing v1/,
      }),
    ).toHaveCount(0);

    await page
      .getByRole("button", { name: "Open navigation menu" })
      .click();
    const navigation = page.getByRole("dialog", { name: "Navigation menu" });
    const search = navigation.getByRole("combobox", {
      name: "search markets",
    });
    await expect(search).toBeFocused();
    const dropdown = navigation.locator(".nav-search-dropdown");
    await expect(dropdown).toContainText(
      "no events or markets match “SYB-247”",
    );
    await expect(dropdown.getByRole("option")).toHaveCount(0);
  });

  test("arena filters keep their native controls inside the mobile panel", async ({
    page,
  }) => {
    await proxyApiForLocalRun(page);
    await page.goto("/arena");

    for (const label of [
      "Filter fair value drift by bot",
      "Select fair value drift market",
      "Filter recent decisions by bot",
    ]) {
      const select = page.getByRole("combobox", { name: label });
      await expect(select).toBeVisible({ timeout: 30_000 });
      await expectInsideViewport(select, MOBILE_VIEWPORT.width, label);
    }
  });

  test("leaderboard outage keeps its retry action in the mobile viewport", async ({
    page,
  }) => {
    await proxyApiForLocalRun(page);
    await page.route(`${API_BASE}/v1/leaderboard**`, async (route) => {
      await route.fulfill({
        status: 503,
        contentType: "application/json",
        body: JSON.stringify({ error: "synthetic leaderboard outage" }),
      });
    });
    await page.goto("/leaderboard");

    const alert = page
      .getByRole("alert")
      .filter({ hasText: "leaderboard unavailable" });
    await expect(alert).toContainText("leaderboard unavailable", {
      timeout: 30_000,
    });
    await expect(page.getByText("no ranked traders yet")).toHaveCount(0);

    const retry = alert.getByRole("button", { name: "retry" });
    await expect(retry).toBeVisible();
    const retryBox = await retry.boundingBox();
    expect(retryBox).not.toBeNull();
    expect(retryBox!.x).toBeGreaterThanOrEqual(0);
    expect(retryBox!.x + retryBox!.width).toBeLessThanOrEqual(
      MOBILE_VIEWPORT.width,
    );
    expect(retryBox!.width).toBeGreaterThanOrEqual(43.5);
    expect(retryBox!.height).toBeGreaterThanOrEqual(43.5);
  });

  test("open-batch outage does not relabel the committed price as indicative", async ({
    page,
  }) => {
    await proxyApiForLocalRun(page);
    await page.goto("/");
    const href = await page
      .locator('[data-testid="markets-grid"] a[href^="/m/"]')
      .first()
      .getAttribute("href");
    expect(href).toMatch(/^\/m\/\d+/);
    const marketId = href!.split("/").pop();

    await page.route(
      `${API_BASE}/v1/markets/${marketId}/open-batch`,
      async (route) => {
        await route.fulfill({
          status: 503,
          contentType: "application/json",
          body: JSON.stringify({ error: "synthetic open-batch outage" }),
        });
      },
    );
    await page.goto(href!);
    await page.getByRole("tab", { name: /Pro/ }).click();

    const hero = page.getByTestId("batch-hero");
    await expect(
      hero
        .getByRole("alert")
        .filter({ hasText: "open-batch data unavailable" }),
    ).toBeVisible({ timeout: 30_000 });
    await expect(
      hero.getByText("last clearing price", { exact: true }),
    ).toBeVisible();
    await expect(
      hero.getByText("indicative price", { exact: true }),
    ).toHaveCount(0);
    const retry = hero.getByRole("button", { name: "retry" });
    await expect(retry).toBeVisible();
    const retryBox = await retry.boundingBox();
    expect(retryBox).not.toBeNull();
    expect(retryBox!.width).toBeGreaterThanOrEqual(43.5);
    expect(retryBox!.height).toBeGreaterThanOrEqual(43.5);
  });
});

test.describe("compact desktop nav boundary", () => {
  test.use({ viewport: { width: 1280, height: 800 } });
  test.afterEach(async ({ page }) => {
    await page.unrouteAll({ behavior: "ignoreErrors" });
  });

  test("1280px chrome stays inside the viewport", async ({ page }) => {
    await proxyApiForLocalRun(page);
    await page.goto("/");
    await expect(
      page.getByRole("button", { name: "Open navigation menu" }),
    ).toBeVisible();
    await expectNoDocumentOverflow(page, "/ at 1280px");
  });
});

test.describe("short mobile recovery", () => {
  const SHORT_VIEWPORT = { width: 320, height: 568 };
  test.use({ viewport: SHORT_VIEWPORT, isMobile: true, hasTouch: true });
  test.afterEach(async ({ page }) => {
    await page.unrouteAll({ behavior: "ignoreErrors" });
  });

  test("saved-account import remains reachable inside the locked modal", async ({
    page,
  }) => {
    await page.addInitScript(() => {
      localStorage.setItem("sybil:auth:account_id", "42");
      localStorage.setItem("sybil:auth:pubkey_hex", "04".padEnd(130, "0"));
      localStorage.setItem("sybil:auth:auth_scheme", "raw_p256");
      localStorage.setItem("sybil:auth:private_key_jwk", "{}");
      localStorage.setItem("sybil:auth:revision", "short-mobile-recovery");
    });
    await proxyApiForLocalRun(page);
    await page.goto("/");
    await expect(
      page.getByRole("button", { name: "Open navigation menu" }),
    ).toBeVisible();
    await expectNoDocumentOverflow(page, "/ at 320px");
    await page.getByRole("button", { name: "connect", exact: true }).click();

    const dialog = page.getByRole("dialog", { name: "Connect" });
    await expect(dialog).toBeVisible();
    await dialog.getByRole("button", { name: "Import existing" }).click();

    const card = page.getByTestId("connect-modal-card");
    const cardBox = await card.boundingBox();
    expect(cardBox).not.toBeNull();
    expect(cardBox!.y).toBeGreaterThanOrEqual(0);
    expect(cardBox!.y + cardBox!.height).toBeLessThanOrEqual(
      SHORT_VIEWPORT.height,
    );

    const content = page.getByTestId("connect-modal-content");
    await expect(content).toHaveCSS("overflow-y", "auto");
    const importButton = dialog.getByRole("button", {
      name: "Import",
      exact: true,
    });
    await dialog.getByRole("button", { name: "Close" }).focus();
    await page.keyboard.press("Shift+Tab");
    await expect(importButton).toBeFocused();
    await expect(importButton).toBeVisible();
    const importBox = await importButton.boundingBox();
    expect(importBox).not.toBeNull();
    expect(importBox!.y).toBeGreaterThanOrEqual(0);
    expect(importBox!.y + importBox!.height).toBeLessThanOrEqual(
      SHORT_VIEWPORT.height,
    );
    expect(importBox!.width).toBeGreaterThanOrEqual(43.5);
    expect(importBox!.height).toBeGreaterThanOrEqual(43.5);
    await expect(page.locator("body")).toHaveCSS("overflow", "hidden");
  });

  test("arena filters remain usable at 320px", async ({ page }) => {
    await proxyApiForLocalRun(page);
    await page.goto("/arena");

    for (const label of [
      "Filter fair value drift by bot",
      "Select fair value drift market",
      "Filter recent decisions by bot",
    ]) {
      const select = page.getByRole("combobox", { name: label });
      await expect(select).toBeVisible({ timeout: 30_000 });
      await expectInsideViewport(select, SHORT_VIEWPORT.width, label);
    }
  });
});

async function gridColumnCount(grid: Locator): Promise<number> {
  return grid.evaluate((node) => {
    const columns = getComputedStyle(node).gridTemplateColumns.trim();
    return columns === "none" || columns === ""
      ? 0
      : columns.split(/\s+/).length;
  });
}

async function expectNoDocumentOverflow(page: Page, path: string) {
  await expect
    .poll(
      () =>
        page.evaluate(
          () =>
            Math.max(
              document.documentElement.scrollWidth,
              document.body.scrollWidth,
            ) - window.innerWidth,
        ),
      { message: `${path} should not overflow the document horizontally` },
    )
    .toBeLessThanOrEqual(1);
}

async function expectInsideViewport(
  locator: Locator,
  viewportWidth: number,
  label: string,
) {
  const box = await locator.boundingBox();
  expect(box, `${label} should have a layout box`).not.toBeNull();
  expect(
    box!.x,
    `${label} should start inside the viewport`,
  ).toBeGreaterThanOrEqual(0);
  expect(
    box!.x + box!.width,
    `${label} should end inside the viewport`,
  ).toBeLessThanOrEqual(viewportWidth);
}

async function expectTouchTargets(page: Page, path: string) {
  const undersized = await page
    .locator("button:visible, a[href]:visible")
    .evaluateAll((targets) =>
      targets.flatMap((target) => {
        if (target.getAttribute("aria-label") === "Open Next.js Dev Tools") {
          return [];
        }
        if (
          target instanceof HTMLAnchorElement &&
          getComputedStyle(target).display === "inline"
        ) {
          return [];
        }
        const rect = target.getBoundingClientRect();
        if (rect.width >= 43.5 && rect.height >= 43.5) return [];
        return [
          `${target.getAttribute("aria-label") ?? target.textContent?.trim() ?? target.tagName.toLowerCase()} (${rect.width.toFixed(1)}×${rect.height.toFixed(1)})`,
        ];
      }),
    );
  expect(undersized, `${path} should expose 44px touch targets`).toEqual([]);
}

/** The deployed API only allows the deployed app origin. For a local visual
 * run, proxy API responses through Playwright's request context so the smoke
 * test exercises local UI code without weakening production CORS. */
async function proxyApiForLocalRun(page: Page) {
  const host = new URL(APP_BASE).hostname;
  if (host !== "127.0.0.1" && host !== "localhost") return;

  await page.route(`${API_BASE}/**`, async (route) => {
    // Playwright 1.61 on Node 24 can receive an empty peer-certificate object
    // in its API request context, then crash while reading subject.CN. Keep
    // that transport out of this local-only CORS bridge; production/browser
    // networking is unchanged.
    const request = route.request();
    const method = request.method();
    const init: RequestInit = {
      method,
      headers: request.headers(),
    };
    const postData = request.postData();
    if (!["GET", "HEAD"].includes(method) && postData != null) {
      init.body = postData;
    }
    const response = await fetch(request.url(), init);
    const headers = Object.fromEntries(response.headers);
    // Native fetch decodes compressed bodies but retains these wire headers.
    // Forwarding them would make the browser try to decode the plain body again.
    delete headers["content-encoding"];
    delete headers["content-length"];
    delete headers["transfer-encoding"];
    await route.fulfill({
      status: response.status,
      headers,
      body: Buffer.from(await response.arrayBuffer()),
    });
  });
}
