import { expect, test, type Page } from "@playwright/test";

const APP_BASE = process.env.E2E_BASE_URL ?? "https://app.sybil.exchange";
const API_BASE = process.env.E2E_API_BASE ?? "https://api.sybil.exchange";

test.describe("compact global navigation", () => {
  test.use({ viewport: { width: 390, height: 844 } });
  test.afterEach(async ({ page }) => {
    await page.unrouteAll({ behavior: "ignoreErrors" });
  });

  test("is a keyboard-safe sheet and does not leak background scroll", async ({
    page,
  }) => {
    await proxyApiForLocalRun(page);
    await page.goto("/settings");

    // Give the document a deterministic scroll range independent of the API's
    // current market count so a leaked wheel gesture would move the page.
    await page.evaluate(() => {
      const spacer = document.createElement("div");
      spacer.setAttribute("data-test-scroll-range", "");
      spacer.style.height = "3000px";
      document.body.append(spacer);
    });

    const trigger = page.getByRole("button", {
      name: "Open navigation menu",
    });
    await trigger.focus();
    await page.keyboard.press("Enter");

    const sheet = page.getByRole("dialog", { name: "Navigation menu" });
    await expect(sheet).toBeVisible();
    await expect(sheet).toHaveAttribute("aria-modal", "true");
    await expect(
      sheet.getByRole("combobox", { name: "search markets" }),
    ).toBeFocused();
    await expect(page.locator("body")).toHaveCSS("overflow", "hidden");

    const scrollBeforeWheel = await page.evaluate(() => window.scrollY);
    await page.mouse.wheel(0, 500);
    await expect
      .poll(() => page.evaluate(() => window.scrollY))
      .toBe(scrollBeforeWheel);

    const focusable = sheet.locator(
      'a[href], button:not([disabled]), input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])',
    );
    const first = focusable.first();
    const last = focusable.last();

    await last.focus();
    await page.keyboard.press("Tab");
    await expect(first).toBeFocused();

    await first.focus();
    await page.keyboard.press("Shift+Tab");
    await expect(last).toBeFocused();

    await page.keyboard.press("Escape");
    await expect(sheet).toBeHidden();
    await expect(trigger).toBeFocused();
    await expect(page.locator("body")).not.toHaveCSS("overflow", "hidden");
    expect(await page.evaluate(() => window.scrollY)).toBe(scrollBeforeWheel);
  });
});

/** The deployed API only allows the deployed app origin. For a local run,
 * proxy browser API traffic without changing production CORS. */
async function proxyApiForLocalRun(page: Page) {
  const host = new URL(APP_BASE).hostname;
  if (host !== "127.0.0.1" && host !== "localhost") return;

  await page.route(`${API_BASE}/**`, async (route) => {
    const response = await route.fetch();
    await route.fulfill({ response });
  });
}
