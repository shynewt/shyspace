import { expect, test } from "@playwright/test";

test("the emoji picker loads its data from the app, never from the network", async ({ page, baseURL }) => {
  const appOrigin = new URL(baseURL ?? "").origin;
  const requestsElsewhere: string[] = [];
  await page.route(
    (url) => url.origin !== appOrigin,
    (route) => {
      requestsElsewhere.push(route.request().url());
      return route.abort();
    },
  );
  await page.goto("");
  await expect(page.getByLabel("Message #general")).toBeVisible();

  await page.getByRole("button", { name: /^Ada\b/ }).click();
  await page.getByRole("menuitem", { name: "Set status" }).click();
  const dialog = page.getByRole("dialog", { name: "Set a status" });
  await dialog.getByRole("button", { name: "Choose an emoji" }).click();
  await page.getByLabel("Search emoji").fill("headphone");
  await page.getByRole("gridcell", { name: "headphone" }).click();

  await expect(dialog.getByRole("button", { name: "Choose an emoji" })).toContainText("🎧");
  expect(requestsElsewhere).toEqual([]);
});

test("a chosen status emoji can be removed again", async ({ page }) => {
  await page.goto("");
  await expect(page.getByLabel("Message #general")).toBeVisible();
  await page.getByRole("button", { name: /^Ada\b/ }).click();
  await page.getByRole("menuitem", { name: "Set status" }).click();
  const dialog = page.getByRole("dialog", { name: "Set a status" });
  const emoji = dialog.getByRole("button", { name: "Choose an emoji" });
  await emoji.click();
  await page.getByLabel("Search emoji").fill("palm tree");
  await page.getByRole("gridcell", { name: "palm tree" }).click();
  await expect(emoji).toContainText("🌴");

  await emoji.click();
  await page.getByRole("button", { name: "Remove emoji" }).click();
  await expect(emoji).not.toContainText("🌴");
});

test("the emoji picker works under the app's connect rules", async ({ page }) => {
  // The packaged app only lets the page connect to the core (tauri.conf.json):
  // the emoji data must not need a request at all.
  await page.route("**/tests/ui/app.html", async (route) => {
    const response = await route.fetch();
    await route.fulfill({
      response,
      headers: { ...response.headers(), "content-security-policy": "connect-src ipc: http://ipc.localhost" },
    });
  });
  await page.goto("");
  await expect(page.getByLabel("Message #general")).toBeVisible();

  await page.getByRole("button", { name: /^Ada\b/ }).click();
  await page.getByRole("menuitem", { name: "Set status" }).click();
  await page.getByRole("dialog", { name: "Set a status" }).getByRole("button", { name: "Choose an emoji" }).click();
  await page.getByLabel("Search emoji").fill("rocket");
  await expect(page.getByRole("gridcell", { name: "rocket" })).toBeVisible();
});
