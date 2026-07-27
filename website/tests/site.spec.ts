import { expect, test } from "@playwright/test";

test("homepage leads into the documentation", async ({ page }) => {
  await page.goto("./");
  await expect(page).toHaveTitle(/Caelix/);
  await page.getByRole("link", { name: /Get started/ }).click();
  await expect(page).toHaveURL(/\/caelix\/getting-started\/overview\/$/);
});

test("homepage links to the Why Caelix guide", async ({ page }) => {
  await page.goto("./");
  await page
    .getByRole("link", { name: /Choose your backend boundary/ })
    .click();
  await expect(page).toHaveURL(/\/caelix\/why-caelix\/overview\/$/);
  await expect(page.getByRole("heading", { name: "Why Caelix" })).toBeVisible();
});

test("theme choice persists and code blocks can be copied", async ({
  page,
}) => {
  await page.goto("getting-started/minimal-application/");
  await page
    .getByRole("button", { name: /Use dark theme|Use light theme/ })
    .click();
  await expect(page.locator("html")).toHaveAttribute("data-theme", "dark");
  await page.reload();
  await expect(page.locator("html")).toHaveAttribute("data-theme", "dark");
  await page.locator(".copy-code").first().click();
  await expect(page.locator(".copy-code").first()).toHaveText("Copied");
});

test("search returns local documentation results", async ({ page }) => {
  await page.goto("./");
  await page.keyboard.press(
    process.platform === "darwin" ? "Meta+k" : "Control+k",
  );
  await page.getByRole("searchbox").fill("providers");
  await expect(page.locator(".search-result").first()).toContainText(
    /Providers/i,
  );
});

test("mobile navigation opens the documentation sidebar", async ({
  page,
}, testInfo) => {
  test.skip(testInfo.project.name !== "mobile", "Mobile-only behavior");
  await page.goto("concepts/modules/");
  const toggle = page.getByRole("button", {
    name: "Open documentation navigation",
  });
  await toggle.click();
  await expect(page.locator("[data-docs-sidebar]")).toHaveAttribute(
    "data-open",
    "true",
  );
});
