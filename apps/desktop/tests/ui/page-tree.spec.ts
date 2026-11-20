import { expect, type Locator, type Page, test } from "@playwright/test";

const sidebar = (page: Page) => page.getByRole("navigation", { name: "Workspace" });
const pageItem = (page: Page, title: string) => sidebar(page).getByRole("button", { name: title, exact: true });

async function iconLeft(item: Locator): Promise<number> {
  const box = await item.locator("svg").first().boundingBox();
  if (!box) throw new Error("the page item has no icon on screen");
  return box.x;
}

test.beforeEach(async ({ page }) => {
  await page.goto("");
});

test("a sub-page sits deeper than its parent, and top-level pages line up", async ({ page }) => {
  await page.evaluate(() => {
    window.fake.seedPage("TEST", "");
    window.fake.seedPage("Test2", "", "TEST");
    window.fake.seedPage("Loose", "");
  });
  await expect(pageItem(page, "Test2")).toBeVisible();

  // Polled: the sidebar animates in, so positions settle after a moment.
  const offsetFromParent = async (title: string) =>
    (await iconLeft(pageItem(page, title))) - (await iconLeft(pageItem(page, "TEST")));
  await expect.poll(() => offsetFromParent("Loose")).toBe(0);
  expect(await offsetFromParent("Test2")).toBeGreaterThanOrEqual(12);
});

test("a page's hover actions never cover its name", async ({ page }) => {
  const title = "A page with a name far too long for the sidebar";
  await page.evaluate((title) => window.fake.seedPage(title, ""), title);
  await pageItem(page, title).hover();

  const actions = sidebar(page).getByRole("button", { name: `New sub-page in ${title}` });
  await expect(actions).toBeVisible();
  const name = await pageItem(page, title).getByText(title).boundingBox();
  const action = await actions.boundingBox();
  if (!name || !action) throw new Error("the name or the actions aren't on screen");
  expect(name.x + name.width).toBeLessThanOrEqual(action.x);
});

test("the Pages section collapses like channel sections", async ({ page }) => {
  await page.evaluate(() => window.fake.seedPage("Notes", ""));
  const header = sidebar(page).getByRole("button", { name: "Pages", exact: true });
  await expect(header).toHaveAttribute("aria-expanded", "true");

  await header.click();
  await expect(header).toHaveAttribute("aria-expanded", "false");
  await expect(pageItem(page, "Notes")).toHaveCount(0);
  await expect(sidebar(page).getByRole("button", { name: "New page" })).toBeAttached();

  await header.click();
  await expect(pageItem(page, "Notes")).toBeVisible();
});

test("a page's actions hide after mouse use, and show for the keyboard", async ({ page }) => {
  await page.evaluate(() => window.fake.seedPage("Notes", ""));
  const addSubpage = sidebar(page).getByRole("button", { name: "New sub-page in Notes" });
  await pageItem(page, "Notes").hover();
  await sidebar(page).getByRole("button", { name: "Actions for Notes" }).click();
  await expect(page.getByRole("menu")).toBeVisible();
  await page.mouse.click(900, 400);
  await expect(page.getByRole("menu")).toHaveCount(0);
  // Folded away to no width: clipped out of view, though still in the page.
  await expect(addSubpage).not.toBeInViewport();

  await pageItem(page, "Notes").focus();
  await page.keyboard.press("Tab");
  await expect(addSubpage).toBeFocused();
  await expect(addSubpage).toBeInViewport();
});
