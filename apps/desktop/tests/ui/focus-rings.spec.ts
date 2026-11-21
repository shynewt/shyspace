import { expect, type Locator, type Page, test } from "@playwright/test";

const PAGE_TITLE = "Engineering handbook";

test.beforeEach(async ({ page }) => {
  await page.goto("");
  await page.evaluate((title) => window.fake.seedPage(title, "How we work"), PAGE_TITLE);
});

function sidebarRow(page: Page, name: string): Locator {
  return page.getByRole("navigation", { name: "Workspace" }).getByRole("button", { name, exact: true });
}

/** Whatever has focus, and wherever it is, draws no ring. */
async function expectNoRing(page: Page) {
  await expect
    .poll(() =>
      page.evaluate(() => {
        const focused = document.activeElement;
        if (!focused?.matches(":focus-visible")) return "none";
        const style = getComputedStyle(focused);
        return style.outlineStyle === "none" && style.boxShadow === "none"
          ? "none"
          : `${focused.tagName} ${focused.textContent?.slice(0, 40)}: ${style.outlineStyle} ${style.boxShadow}`;
      }),
    )
    .toBe("none");
}

async function tabTo(page: Page, target: Locator, key: "Tab" | "Shift+Tab" = "Tab") {
  for (let step = 0; step < 60; step++) {
    if (await target.evaluate((element) => element === document.activeElement)) return;
    await page.keyboard.press(key);
  }
  throw new Error("Tab never reached the row");
}

/** A ring drawn inside the row, so its own box (and the sidebar) can't clip it. */
async function expectInsetRing(row: Locator) {
  await expect(row).toBeFocused();
  await expect(row).toHaveCSS("outline-style", "solid");
  const ring = await row.evaluate((element) => {
    const style = getComputedStyle(element);
    let clipper = element.parentElement;
    while (clipper && getComputedStyle(clipper).overflowX === "visible") clipper = clipper.parentElement;
    const box = element.getBoundingClientRect();
    const clip = (clipper ?? document.documentElement).getBoundingClientRect();
    return {
      width: Number.parseFloat(style.outlineWidth),
      offset: Number.parseFloat(style.outlineOffset),
      isInsideClip:
        box.left >= clip.left && box.right <= clip.right && box.top >= clip.top && box.bottom <= clip.bottom,
    };
  });
  expect(ring.width).toBeGreaterThanOrEqual(1);
  expect(ring.offset + ring.width, "the ring stays inside the row").toBeLessThanOrEqual(0);
  expect(ring.isInsideClip, "the row itself isn't cut off").toBe(true);
}

test("clicking a page row shows no focus ring", async ({ page }) => {
  await sidebarRow(page, PAGE_TITLE).click();
  await expect(page.locator(".cm-content")).toHaveText("How we work");
  await expectNoRing(page);
});

test("clicking a channel row shows no focus ring", async ({ page }) => {
  await sidebarRow(page, "general").click();
  await expect(page.getByLabel("Message #general")).toBeVisible();
  await expectNoRing(page);
});

test("clicking rows after typing in the composer shows no focus ring", async ({ page }) => {
  await sidebarRow(page, "general").click();
  await page.getByLabel("Message #general").pressSequentially("half a thought");

  await sidebarRow(page, PAGE_TITLE).click();
  await expect(page.locator(".cm-content")).toHaveText("How we work");
  await expectNoRing(page);
});

test("clicking rows after typing in the page editor shows no focus ring", async ({ page }) => {
  await sidebarRow(page, PAGE_TITLE).click();
  await page.locator(".cm-content").click();
  await page.keyboard.type(" together");

  await sidebarRow(page, "general").click();
  await expect(page.getByLabel("Message #general")).toBeVisible();
  await expectNoRing(page);

  await page.getByLabel("Message #general").pressSequentially("hi");
  await sidebarRow(page, PAGE_TITLE).click();
  await expect(page.locator(".cm-content")).toContainText("together");
  await expectNoRing(page);
});

test("typing or Esc after clicking a row leaves it without a ring", async ({ page }) => {
  const row = sidebarRow(page, PAGE_TITLE);
  await row.click();
  await expect(row).toBeFocused();
  for (const key of ["Escape", "a"]) {
    await page.keyboard.press(key);
    await expectNoRing(page);
  }

  await page.keyboard.press("Tab");
  await expect(page.locator(":focus")).toHaveCSS("outline-style", "solid");
});

test("arrows with Super, Ctrl or Alt after clicking a row leave it without a ring", async ({ page }) => {
  const row = sidebarRow(page, PAGE_TITLE);
  await row.click();
  await expect(row).toBeFocused();
  // Super+arrows belong to the window manager; Alt+arrows are the app's own back and forward.
  for (const key of ["Meta+ArrowRight", "Meta+ArrowDown", "Control+ArrowDown", "Alt+ArrowLeft", "Alt+ArrowRight"]) {
    await page.keyboard.press(key);
    await expectNoRing(page);
  }
});

test("keys arriving as the window gains focus leave the clicked row without a ring", async ({ page }) => {
  const row = sidebarRow(page, PAGE_TITLE);
  await row.click();
  await expect(row).toBeFocused();

  // A Wayland compositor hands the window the keys still held from the switch.
  await page.evaluate(() => window.dispatchEvent(new FocusEvent("focus")));
  await page.keyboard.down("ArrowRight");
  await expectNoRing(page);
  await page.waitForTimeout(300);
  await page.keyboard.down("ArrowRight");
  await page.keyboard.up("ArrowRight");
  await expectNoRing(page);

  await page.keyboard.press("Tab");
  await expect(page.locator(":focus")).toHaveCSS("outline-style", "solid");
});

test("tabbing onto sidebar rows shows a ring inside each row", async ({ page }) => {
  await sidebarRow(page, "Inbox").click();
  await tabTo(page, sidebarRow(page, "general"));
  await expectInsetRing(sidebarRow(page, "general"));

  await tabTo(page, sidebarRow(page, PAGE_TITLE));
  await expectInsetRing(sidebarRow(page, PAGE_TITLE));
});

test("the selected row shows the inset ring on top of its background", async ({ page }) => {
  const row = sidebarRow(page, PAGE_TITLE);
  await row.click();
  await expect(row).toHaveAttribute("aria-current", "page");
  await tabTo(page, page.getByRole("button", { name: `New sub-page in ${PAGE_TITLE}` }));
  await tabTo(page, row, "Shift+Tab");
  await expectInsetRing(row);
});

test("list rows in the Inbox show the inset ring", async ({ page }) => {
  await sidebarRow(page, PAGE_TITLE).click();
  await page.evaluate(() => {
    const grace = window.fake.addMember("Grace");
    window.fake.receiveMessage(grace, "general", `[@Ada](shy://member/${window.fake.session?.me}) can you look?`);
  });
  await page.getByRole("button", { name: "Inbox, 1 unread" }).click();
  const row = page.getByRole("region", { name: "Mentions and replies" }).getByRole("button", { name: /can you look/ });
  await expect(row).toBeVisible();
  await tabTo(page, row);
  await expectInsetRing(row);
});

test("buttons keep an outer ring at least 2px thick", async ({ page }) => {
  const newPage = page.getByRole("button", { name: "New page" });
  await sidebarRow(page, "Inbox").click();
  await tabTo(page, newPage);
  await expect(newPage).toHaveCSS("outline-style", "solid");
  const ring = await newPage.evaluate((element) => {
    const style = getComputedStyle(element);
    return { width: Number.parseFloat(style.outlineWidth), offset: Number.parseFloat(style.outlineOffset) };
  });
  expect(ring.width).toBeGreaterThanOrEqual(2);
  expect(ring.offset).toBeGreaterThan(0);
});

async function openSettings(page: Page) {
  await expect(sidebarRow(page, "general")).toBeVisible();
  await page.keyboard.press("Control+,");
  return page.getByRole("dialog", { name: "Settings" });
}

/** No panel draws a ring, whether focus is on it or not. */
async function expectNoPanelRing(page: Page) {
  const rings = await page
    .getByRole("tabpanel")
    .evaluateAll((panels) =>
      panels.map((panel) => getComputedStyle(panel).outlineStyle).filter((style) => style !== "none"),
    );
  expect(rings).toEqual([]);
}

test("clicking Settings sections and tabs leaves no ring on the panel", async ({ page }) => {
  const settings = await openSettings(page);
  await settings.getByRole("tab", { name: "Members" }).click();
  await settings.getByRole("tab", { name: "Invites" }).click();
  await settings.getByRole("heading", { name: "Members" }).click();
  for (const key of ["ArrowDown", "Meta+ArrowRight", "a"]) {
    await page.keyboard.press(key);
    await expectNoPanelRing(page);
    await expectNoRing(page);
  }
});

test("Tab from a Settings tab list goes to the panel's first control, not the panel", async ({ page }) => {
  const settings = await openSettings(page);
  await settings.getByRole("tab", { name: "Members" }).click();
  await page.keyboard.press("Tab");
  const focused = () =>
    page.evaluate(() => {
      const element = document.activeElement;
      return {
        role: element?.getAttribute("role") ?? element?.tagName ?? "",
        isInPanel: Boolean(element?.closest("[role=tabpanel]")),
      };
    });
  expect(await focused()).toEqual({ role: "tab", isInPanel: true });

  await settings.getByRole("tab", { name: "Invites" }).click();
  await page.keyboard.press("Tab");
  const control = await focused();
  expect(control.isInPanel).toBe(true);
  expect(control.role).not.toBe("tabpanel");
});

test("clicking between messages and scrolling with keys draws no ring around the list", async ({ page }) => {
  await sidebarRow(page, "general").click();
  const composer = page.getByLabel("Message #general");
  await composer.fill("Hello");
  await composer.press("Enter");
  const log = page.getByRole("log", { name: "Messages in #general" });
  const box = await log.boundingBox();
  if (!box) throw new Error("the message list isn't on screen");
  await page.mouse.click(box.x + box.width / 2, box.y + 4);
  await expect(log).toBeFocused();

  for (const key of ["PageDown", "ArrowUp"]) {
    await page.keyboard.press(key);
    await expect(log).toHaveCSS("outline-style", "none");
  }
});
