import { expect, test } from "@playwright/test";

declare global {
  interface Window {
    /** Lets a held search answer go; resolves once it has. */
    releaseHeldSearch?: () => Promise<void>;
  }
}

test("search finds a message and jumps to its channel", async ({ page }) => {
  await page.goto("");
  await page.evaluate(() => {
    const grace = window.fake.addMember("Grace");
    window.fake.receiveMessage(grace, "general", "The quarterly budget is ready");
  });

  await page.getByRole("button", { name: /Search/ }).click();
  await page.getByLabel("Search messages and pages").fill("budg");
  const hit = page.getByRole("option", { name: /quarterly budget/ });
  await expect(hit).toBeVisible();
  await page.keyboard.press("Enter");

  await expect(page.getByLabel("Message #general")).toBeVisible();
  await expect(page.getByRole("article").getByText("The quarterly budget is ready")).toBeVisible();
});

test("search results say who wrote a message and when a page changed", async ({ page }) => {
  await page.clock.install({ time: new Date(2026, 10, 23, 15, 30) });
  await page.goto("");
  await page.evaluate(() => {
    window.fake.receiveMessage(window.fake.addMember("Grace"), "general", "The quarterly budget is ready");
    window.fake.seedPage("Quarterly plan", "Budget first");
  });
  await expect(page.getByLabel("Message #general")).toBeVisible();

  await page.keyboard.press("Control+k");
  await page.getByLabel("Search messages and pages").fill("quarterly");
  await expect(page.getByRole("option", { name: /quarterly budget/ })).toContainText(/Grace, 3:30\sPM/);
  await expect(page.getByRole("option").filter({ hasText: "Budget first" })).toContainText(/Edited 3:30\sPM/);
});

test("a slow answer for an older query doesn't replace newer results", async ({ page }) => {
  await page.goto("");
  await page.evaluate(() => {
    const grace = window.fake.addMember("Grace");
    window.fake.receiveMessage(grace, "general", "We ship on Friday");
    window.fake.receiveMessage(grace, "general", "The shipping crates arrived");
    const fake = window.fake;
    const handle = fake.handle.bind(fake);
    fake.handle = (command, args) => {
      const answer = handle(command, args);
      const isHeld = command === "search" && typeof args === "object" && "query" in args && args.query === "ship";
      if (!isHeld) return answer;
      return new Promise((resolve) => {
        window.releaseHeldSearch = async () => {
          resolve(answer);
          await new Promise((settled) => setTimeout(settled, 100));
        };
      });
    };
  });
  await expect(page.getByLabel("Message #general")).toBeVisible();

  await page.keyboard.press("Control+k");
  const box = page.getByLabel("Search messages and pages");
  await box.fill("ship");
  await expect.poll(() => page.evaluate(() => window.releaseHeldSearch !== undefined)).toBe(true);
  await box.fill("shipping");
  await expect(page.getByRole("option", { name: /shipping crates/ })).toBeVisible();

  await page.evaluate(() => window.releaseHeldSearch?.());
  await expect(page.getByRole("option", { name: /ship on Friday/ })).toHaveCount(0);
  await expect(page.getByRole("option", { name: /shipping crates/ })).toBeVisible();
});

test("the palette runs actions", async ({ page }) => {
  await page.goto("");
  await expect(page.getByLabel("Message #general")).toBeVisible();
  await page.keyboard.press("Control+k");
  await page.getByLabel("Search messages and pages").fill("inbox");
  await page.getByRole("option", { name: /Open inbox/ }).click();

  await expect(page.getByRole("heading", { name: "Inbox" })).toBeVisible();
});

test("a reply found by search opens in its thread", async ({ page }) => {
  await page.goto("");
  await page.evaluate(() => {
    const grace = window.fake.addMember("Grace");
    const question = window.fake.receiveMessage(grace, "general", "Which version ships?");
    window.fake.receiveReply(grace, question, "Version two ships Friday");
  });
  await expect(page.getByLabel("Message #general")).toBeVisible();

  await page.keyboard.press("Control+k");
  await page.getByLabel("Search messages and pages").fill("Friday");
  await page.getByRole("option", { name: /Version two/ }).click();

  const thread = page.getByRole("complementary", { name: "Thread" });
  await expect(thread.getByRole("article").filter({ hasText: "Version two ships Friday" })).toBeVisible();
});

test("marking all as read from the palette asks first", async ({ page }) => {
  await page.goto("");
  await expect(page.getByLabel("Message #general")).toBeVisible();
  await page.keyboard.press("Control+k");
  await page.getByLabel("Search messages and pages").fill("mark all");
  await page.getByRole("option", { name: /Mark all as read/ }).click();

  await expect(page.getByRole("dialog", { name: "Mark everything as read?" })).toBeVisible();
});

test("arrows held with Ctrl or Super don't move through the results", async ({ page }) => {
  await page.goto("");
  await expect(page.getByLabel("Message #general")).toBeVisible();
  await page.keyboard.press("Control+k");
  await expect(page.getByLabel("Search messages and pages")).toBeFocused();
  const options = page.locator("#palette-results").getByRole("option");
  await expect(options.nth(1)).toBeVisible();
  await expect(options.first()).toHaveAttribute("aria-selected", "true");

  for (const key of ["Meta+ArrowDown", "Control+ArrowDown"]) await page.keyboard.press(key);

  await expect(options.first()).toHaveAttribute("aria-selected", "true");
});

test("search hits name a DM after the person and show a private channel's lock", async ({ page }) => {
  await page.goto("");
  await page.evaluate(() => {
    const grace = window.fake.addMember("Grace");
    const dm = window.fake.handle("open_dm", { members: [grace] });
    if (typeof dm === "string") window.fake.receiveMessageIn(grace, dm, "The zebra plan, between us");
    window.fake.receivePrivateChannel(grace, "ops");
    window.fake.receiveMessage(grace, "ops", "The zebra plan, for ops");
  });
  await expect(page.getByLabel("Message #general")).toBeVisible();

  await page.keyboard.press("Control+k");
  await page.getByLabel("Search messages and pages").fill("zebra");
  const inDm = page.getByRole("option", { name: /between us/ });
  await expect(inDm.getByText("Grace", { exact: true })).toBeVisible();
  await expect(inDm).not.toContainText("#");
  const inPrivate = page.getByRole("option", { name: /for ops/ });
  await expect(inPrivate.getByLabel("Private", { exact: true })).toBeVisible();
  await expect(inPrivate).toContainText("ops");
  await expect(inPrivate).not.toContainText("#ops");
});
