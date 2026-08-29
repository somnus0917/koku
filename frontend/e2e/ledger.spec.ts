import { expect, test } from "@playwright/test";

test("signs in and records an expense through the full stack", async ({ page }) => {
  await page.addInitScript(() => window.localStorage.setItem("koku-lang", "en"));
  await page.goto("/");

  await page.getByRole("textbox", { name: "Email" }).fill("e2e@example.com");
  await page.locator('input[type="password"]').fill("koku-e2e-password");
  await page.getByRole("button", { name: "Sign in securely" }).click();

  await expect(page.getByRole("heading", { name: "Keep your life in order, one day at a time." })).toBeVisible();
  await page.getByRole("button", { name: "Add entry" }).first().click();

  const dialog = page.getByRole("dialog", { name: "Add entry" });
  await dialog.getByLabel(/Expense amount/).fill("12.50");
  await dialog.getByLabel("Note").fill("Playwright coffee");
  await dialog.getByRole("button", { name: "Confirm entry" }).click();

  await expect(page.getByRole("status", { name: "" }).filter({ hasText: "Transaction recorded" })).toBeVisible();
  await page.reload();
  await page.getByRole("button", { name: "Transactions" }).first().click();
  await expect(page.getByText("Playwright coffee")).toBeVisible();
});
