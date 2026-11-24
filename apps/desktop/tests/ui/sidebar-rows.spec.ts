import { expect, type Locator, type Page, test } from "@playwright/test";

/** A row's hover highlight matches its selected one: the same width and shape, "…" included. */

const sidebar = (page: Page) => page.getByRole("navigation", { name: "Workspace" });

/** The widest painted background inside a sidebar row, as { x, width, radius }. */
async function highlightOf(row: Locator) {
  return row.locator("xpath=..").evaluate((container) => {
    let widest: { x: number; width: number; radius: string } | null = null;
    for (const element of [container, ...container.querySelectorAll("*")]) {
      const style = getComputedStyle(element);
      const isPainted = style.backgroundColor !== "rgba(0, 0, 0, 0)" && style.backgroundColor !== "transparent";
      const box = element.getBoundingClientRect();
      if (!isPainted || box.height < 24 || Number(style.opacity) === 0) continue;
      if (!widest || box.width > widest.width)
        widest = { x: box.x, width: box.width, radius: style.borderTopLeftRadius };
    }
    return widest;
  });
}

test("hovering a row highlights as much of it as selecting it does", async ({ page }) => {
  await page.goto("");
  await expect(page.getByLabel("Message #general")).toBeVisible();
  await page.evaluate(() => window.fake.handle("create_channel", { name: "random" }));
  const general = sidebar(page).getByRole("button", { name: "general", exact: true });
  const random = sidebar(page).getByRole("button", { name: "random", exact: true });
  await general.click();
  const selected = await highlightOf(general);

  await random.hover();
  await expect.poll(async () => (await highlightOf(random))?.width ?? 0).toBeGreaterThan(0);
  const hovered = await highlightOf(random);
  expect(selected).not.toBeNull();
  expect(hovered?.x).toBeCloseTo(selected?.x ?? -1, 0);
  expect(hovered?.width).toBeCloseTo(selected?.width ?? -1, 0);
  expect(hovered?.radius).toBe(selected?.radius);
});
