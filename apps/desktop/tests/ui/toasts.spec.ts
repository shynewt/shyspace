import { expect, test } from "@playwright/test";
import { failWith } from "./support";

test.beforeEach(async ({ page }) => {
  await page.goto("");
  await page.getByRole("button", { name: "general", exact: true }).click();
  const composer = page.getByLabel("Message #general");
  await composer.fill("Shipped it");
  await composer.press("Enter");
  await failWith(page, "react", "the bucket can't be reached");
  const message = page.getByRole("article").filter({ hasText: "Shipped it" });
  await message.hover();
  await message.getByRole("button", { name: "React" }).click();
  await page.getByRole("button", { name: "React with 👍" }).click();
});

test("errors are announced as alerts", async ({ page }) => {
  await expect(page.getByRole("alert")).toContainText("the bucket can't be reached");
});

test("toasts keep clear of the message box", async ({ page }) => {
  const toast = await page.getByRole("alert").boundingBox();
  const composer = await page.getByLabel("Message #general").boundingBox();
  if (!toast || !composer) throw new Error("not laid out");
  expect(toast.y + toast.height).toBeLessThanOrEqual(composer.y);
});
