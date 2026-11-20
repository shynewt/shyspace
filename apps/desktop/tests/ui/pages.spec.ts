import { expect, test } from "@playwright/test";
import { failWith } from "./support";

test("a new page can be titled and written in", async ({ page }) => {
  await page.goto("");
  await page.getByRole("button", { name: "New page" }).click();

  const title = page.getByLabel("Page title");
  await title.fill("Roadmap");
  await title.press("Enter");
  await expect(page.getByRole("navigation", { name: "Workspace" }).getByText("Roadmap")).toBeVisible();

  await page.locator(".cm-content").click();
  await page.keyboard.type("# Q1\nShip the desktop app");

  await expect.poll(() => page.evaluate(() => window.fake.pageText("Roadmap"))).toBe("# Q1\nShip the desktop app");
});

test("an open page reloads when its content is rebuilt", async ({ page }) => {
  await page.goto("");
  await page.getByRole("button", { name: "New page" }).click();
  const title = page.getByLabel("Page title");
  await title.fill("Notes");
  await title.press("Enter");
  await page.locator(".cm-content").click();
  await page.keyboard.type("draft from a removed member");
  await expect.poll(() => page.evaluate(() => window.fake.pageText("Notes"))).toBe("draft from a removed member");

  await page.evaluate(() => window.fake.rebuildPage("Notes", "what everyone agrees on"));

  await expect(page.locator(".cm-content")).toHaveText("what everyone agrees on");
});

test("a page change that can't be saved says why and goes back to the saved page", async ({ page }) => {
  await page.goto("");
  await page.getByRole("button", { name: "New page" }).click();
  const title = page.getByLabel("Page title");
  await title.fill("Notes");
  await title.press("Enter");
  await page.locator(".cm-content").click();
  await page.keyboard.type("saved");
  await expect.poll(() => page.evaluate(() => window.fake.pageText("Notes"))).toBe("saved");

  const tooLarge = "this change is too large to save (over 4 MB); paste it in smaller parts";
  await failWith(page, "apply_doc_update", tooLarge);
  await page.keyboard.type(" never saved");

  await expect(page.getByText(tooLarge)).toBeVisible();
  await expect(page.locator(".cm-content")).toHaveText("saved");
});

test("edits typed while a save is still going out wait for it, and are dropped if it fails", async ({ page }) => {
  await page.goto("");
  await page.getByRole("button", { name: "New page" }).click();
  const title = page.getByLabel("Page title");
  await title.fill("Notes");
  await title.press("Enter");
  await page.locator(".cm-content").click();
  await page.keyboard.type("saved");
  await expect.poll(() => page.evaluate(() => window.fake.pageText("Notes"))).toBe("saved");
  const sends = await page.evaluateHandle(() => {
    const held = { count: 0, fail: () => {} };
    const answer = window.fake.handle.bind(window.fake);
    window.fake.handle = (asked, args) => {
      if (asked !== "apply_doc_update") return answer(asked, args);
      held.count += 1;
      return new Promise((_, reject) => {
        held.fail = () => reject("this change is too large to save");
      });
    };
    return held;
  });

  await page.keyboard.type(" big paste");
  await expect.poll(() => sends.evaluate((held) => held.count)).toBe(1);
  await page.keyboard.type(" typed meanwhile");
  await page.waitForTimeout(400);
  expect(await sends.evaluate((held) => held.count)).toBe(1);
  await sends.evaluate((held) => held.fail());

  await expect(page.locator(".cm-content")).toHaveText("saved");
  expect(await sends.evaluate((held) => held.count)).toBe(1);
});

test("the header shows the page's path, and each part opens", async ({ page }) => {
  await page.goto("");
  await page.evaluate(() => {
    window.fake.seedPage("TEST", "");
    window.fake.seedPage("Test2", "", "TEST");
  });
  await page.getByRole("navigation", { name: "Workspace" }).getByRole("button", { name: "Test2", exact: true }).click();
  const path = page.getByRole("navigation", { name: "Breadcrumb" });
  await expect(path.getByRole("listitem")).toHaveText(["TEST", "Test2"]);

  await path.getByRole("button", { name: "Test2", exact: true }).click();
  await expect(page.getByLabel("Page title")).toBeFocused();

  await path.getByRole("button", { name: "TEST", exact: true }).click();
  await expect(page.getByLabel("Page title")).toHaveValue("TEST");
  await expect(path.getByRole("listitem")).toHaveText(["TEST"]);
});

test("saving a page for later is a bookmark toggle", async ({ page }) => {
  await page.goto("");
  await page.evaluate(() => window.fake.seedPage("Spec", ""));
  await page.getByRole("navigation", { name: "Workspace" }).getByRole("button", { name: "Spec", exact: true }).click();

  await page.getByRole("button", { name: "Save for later" }).click();
  const saved = page.getByRole("button", { name: "Saved for later" });
  await expect(saved).toHaveAttribute("aria-pressed", "true");
  await saved.click();
  await expect(page.getByRole("button", { name: "Save for later" })).toHaveAttribute("aria-pressed", "false");
});

test("rare page actions wait in the more menu", async ({ page }) => {
  await page.goto("");
  await page.evaluate(() => window.fake.seedPage("Spec", ""));
  await page.getByRole("navigation", { name: "Workspace" }).getByRole("button", { name: "Spec", exact: true }).click();
  await expect(page.getByRole("button", { name: "Delete", exact: true })).toHaveCount(0);

  await page.getByRole("button", { name: "More page actions" }).click();
  await expect(page.getByRole("menuitem")).toHaveText(["New sub-page", "Delete page"]);
});

test("a new page starts in its empty title, and Enter moves on to the body", async ({ page }) => {
  await page.goto("");
  await page.getByRole("button", { name: "New page" }).click();
  await expect(page.locator(".cm-content")).toBeVisible();
  const title = page.getByLabel("Page title");
  await expect(title).toBeFocused();

  await page.keyboard.type("Plans");
  await page.keyboard.press("Enter");
  await expect(page.locator(".cm-content")).toBeFocused();
  await expect(title).toHaveValue("Plans");
});

test("a long title wraps instead of being cut off", async ({ page }) => {
  await page.goto("");
  const long = "A page title long enough that it can't possibly fit on a single line of the page";
  await page.evaluate((long) => window.fake.seedPage(long, ""), long);
  await page.getByRole("navigation", { name: "Workspace" }).getByRole("button", { name: long, exact: true }).click();
  const title = page.getByLabel("Page title", { exact: true });
  await expect(title).toHaveValue(long);

  const size = await title.evaluate((field) => ({ content: field.scrollHeight, shown: field.clientHeight }));
  expect(size.shown).toBeGreaterThan(80);
  expect(size.content).toBeLessThanOrEqual(size.shown);
});

test("the empty title's placeholder is readable", async ({ page }) => {
  await page.goto("");
  await page.getByRole("button", { name: "New page" }).click();
  const ratio = await page.getByLabel("Page title").evaluate((field) => {
    const channels = (color: string) => (color.match(/[\d.]+/g) ?? []).slice(0, 3).map(Number);
    const luminance = (color: string) => {
      const [red, green, blue] = channels(color).map((value) => {
        const share = value / 255;
        return share <= 0.03928 ? share / 12.92 : ((share + 0.055) / 1.055) ** 2.4;
      });
      return 0.2126 * red + 0.7152 * green + 0.0722 * blue;
    };
    let surface: Element | null = field;
    while (surface && getComputedStyle(surface).backgroundColor === "rgba(0, 0, 0, 0)") surface = surface.parentElement;
    const text = luminance(getComputedStyle(field, "::placeholder").color);
    const background = luminance(surface ? getComputedStyle(surface).backgroundColor : "rgb(255, 255, 255)");
    return (Math.max(text, background) + 0.05) / (Math.min(text, background) + 0.05);
  });
  expect(ratio).toBeGreaterThanOrEqual(4.5);
});
