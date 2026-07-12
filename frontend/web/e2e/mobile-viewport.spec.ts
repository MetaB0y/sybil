import { expect, test, type Locator, type Page } from "@playwright/test";

const MOBILE_VIEWPORT = { width: 390, height: 844 };
const APP_BASE = process.env.E2E_BASE_URL ?? "https://app.172-104-31-54.nip.io";
const API_BASE =
  process.env.E2E_API_BASE ?? "https://172-104-31-54.nip.io";

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
    await expectTouchButtons(page, "/");

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
    await expectTouchButtons(page, `${href!} order sheet`);

    await closeOrder.click();
    await expect(dialog).toBeHidden();
    await expect(placeOrder).toBeFocused();

    for (const path of ["/portfolio", "/activity", "/arena"]) {
      await page.goto(path);
      await expect(page.locator("main")).toBeVisible();
      await expectNoDocumentOverflow(page, path);
      await expectTouchButtons(page, path);
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

async function gridColumnCount(
  grid: Locator,
): Promise<number> {
  return grid.evaluate((node) => {
    const columns = getComputedStyle(node).gridTemplateColumns.trim();
    return columns === "none" || columns === "" ? 0 : columns.split(/\s+/).length;
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

async function expectTouchButtons(page: Page, path: string) {
  const undersized = await page.locator("button:visible").evaluateAll((buttons) =>
    buttons.flatMap((button) => {
      if (button.getAttribute("aria-label") === "Open Next.js Dev Tools") {
        return [];
      }
      const rect = button.getBoundingClientRect();
      if (rect.width >= 43.5 && rect.height >= 43.5) return [];
      return [
        `${button.getAttribute("aria-label") ?? button.textContent?.trim() ?? "button"} (${rect.width.toFixed(1)}×${rect.height.toFixed(1)})`,
      ];
    }),
  );
  expect(undersized, `${path} should expose 44px touch buttons`).toEqual([]);
}

/** The deployed API only allows the deployed app origin. For a local visual
 * run, proxy API responses through Playwright's request context so the smoke
 * test exercises local UI code without weakening production CORS. */
async function proxyApiForLocalRun(page: Page) {
  const host = new URL(APP_BASE).hostname;
  if (host !== "127.0.0.1" && host !== "localhost") return;

  await page.route(`${API_BASE}/**`, async (route) => {
    const response = await route.fetch();
    await route.fulfill({ response });
  });
}
