import { expect, type Page, test } from "@playwright/test";

function channelLog(page: Page) {
  return page.getByRole("log", { name: "Messages in #general" });
}

function card(page: Page, name: string) {
  return page.getByRole("dialog", { name: `${name}'s profile` });
}

function nameButton(page: Page, name: string) {
  return channelLog(page).getByRole("button", { name, exact: true });
}

test.beforeEach(async ({ page }) => {
  await page.goto("");
  await expect(page.getByLabel("Message #general")).toBeVisible();
  await page.evaluate(() => {
    const grace = window.fake.addMember("Grace");
    window.fake.setPresence(grace, "online");
    window.fake.setMemberStatus(grace, { emoji: "🎧", text: "Focusing", until_ms: null, alerts: "mentions" });
    window.fake.receiveMessage(grace, "general", "Morning all");
  });
  await expect(channelLog(page).getByText("Morning all")).toBeVisible();
});

test("the keyboard opens a card, uses it and closes it", async ({ page }) => {
  await nameButton(page, "Grace").focus();
  await page.keyboard.press("Enter");
  const grace = card(page, "Grace");
  await expect(grace).toBeVisible();
  await expect(grace).toContainText("Online");
  await expect(grace).toContainText("🎧 Focusing");
  await expect(grace).toContainText("Mentions only");
  await expect(grace.getByRole("button", { name: "Message" })).toBeFocused();

  await page.keyboard.press("Escape");
  await expect(grace).toHaveCount(0);
  await expect(nameButton(page, "Grace")).toBeFocused();

  await page.keyboard.press("Space");
  await expect(grace.getByRole("button", { name: "Message" })).toBeFocused();
  await page.keyboard.press("Tab");
  await expect(grace.getByRole("button", { name: "Mention" })).toBeFocused();
  await page.keyboard.press("Enter");
  await expect(grace).toHaveCount(0);
  const composer = page.getByLabel("Message #general");
  await expect(composer).toBeFocused();
  await expect(composer).toHaveValue("@Grace ");
  await page.keyboard.type("are you free?");
  await page.keyboard.press("Enter");
  await expect(channelLog(page).locator(".mention").filter({ hasText: "@Grace" })).toBeVisible();
});

test("clicking an avatar opens the card, and clicking outside closes it", async ({ page }) => {
  await channelLog(page).getByRole("button", { name: "Show Grace's profile" }).click();
  await expect(card(page, "Grace")).toBeVisible();

  await page.getByRole("log", { name: "Messages in #general" }).click({ position: { x: 600, y: 300 } });
  await expect(card(page, "Grace")).toHaveCount(0);
});

test("a pause on a name opens the card, and moving onto it keeps it open", async ({ page }) => {
  const name = nameButton(page, "Grace");
  await name.hover();
  const grace = card(page, "Grace");
  await expect(grace).toBeVisible();
  await expect(page.getByLabel("Message #general")).toBeFocused();

  const box = await grace.boundingBox();
  if (!box) throw new Error("no box for the card");
  // One move from the name onto the card: in steps, a busy machine can leave the pointer in the gap for longer
  // than the close delay, which a person's hand never does.
  await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
  await page.waitForTimeout(700);
  await expect(grace).toBeVisible();

  await page.mouse.move(box.x + box.width + 200, box.y + box.height + 200, { steps: 4 });
  await expect(grace).toHaveCount(0);
  await expect(page.getByLabel("Message #general")).toBeFocused();
});

test("passing quickly over a name opens nothing", async ({ page }) => {
  // The pointer crosses the name within one moment. Two real mouse moves can arrive 700 ms apart on a busy
  // machine (WebKit in a container), which is a pause, not a pass.
  await nameButton(page, "Grace").evaluate((name) => {
    const crossing = { bubbles: true, pointerType: "mouse", isPrimary: true };
    name.dispatchEvent(new PointerEvent("pointerover", { ...crossing, relatedTarget: document.body }));
    name.dispatchEvent(new PointerEvent("pointerout", { ...crossing, relatedTarget: document.body }));
  });
  await page.waitForTimeout(1000);

  await expect(card(page, "Grace")).toHaveCount(0);
});

test("an agent's card says it's an agent and shows no presence or status", async ({ page }) => {
  await page.evaluate(() => {
    const agent = window.fake.addMember("Claude", { kind: "bot" });
    window.fake.setPresence(agent, "online");
    window.fake.setMemberStatus(agent, { emoji: "🤖", text: "Working", until_ms: null, alerts: "all" });
    window.fake.receiveMessage(agent, "general", "Summary ready");
  });
  await nameButton(page, "Claude").click();

  const claude = card(page, "Claude");
  await expect(claude).toContainText("Agent");
  await expect(claude).not.toContainText("Online");
  await expect(claude).not.toContainText("Working");
});

test("a mention in a message opens the card on click, never on hover", async ({ page }) => {
  await page.evaluate(() => {
    const grace = window.fake.memberNamed("Grace");
    window.fake.receiveMessage(grace, "general", `Ask [@Grace](shy://member/${grace}) about it`);
  });
  const mention = channelLog(page).getByRole("button", { name: "@Grace" });
  await mention.hover();
  await page.waitForTimeout(1000);
  await expect(card(page, "Grace")).toHaveCount(0);

  await mention.click();
  await expect(card(page, "Grace")).toBeVisible();
});

test("a mention of someone unknown opens no card", async ({ page }) => {
  await page.evaluate(() =>
    window.fake.receiveMessage(
      window.fake.memberNamed("Grace"),
      "general",
      `Ask [@Bob](shy://member/${"b".repeat(64)})`,
    ),
  );
  await channelLog(page).getByText("@Bob").click();
  await page.waitForTimeout(300);

  await expect(page.getByRole("dialog")).toHaveCount(0);
});

test("a card opened by pausing, then clicked, gives focus back to the name on Esc", async ({ page }) => {
  const name = nameButton(page, "Grace");
  await name.hover();
  await expect(card(page, "Grace")).toBeVisible();
  await name.click();
  await expect(card(page, "Grace")).toBeFocused();

  await page.keyboard.press("Escape");
  await expect(card(page, "Grace")).toHaveCount(0);
  await expect(name).toBeFocused();
});

test("message opens a DM with them, as a step you can go back from", async ({ page }) => {
  await nameButton(page, "Grace").click();
  await card(page, "Grace").getByRole("button", { name: "Message" }).click();

  await expect(page.getByLabel("Message Grace")).toBeVisible();
  await page.keyboard.press("Alt+ArrowLeft");
  await expect(page.getByLabel("Message #general")).toBeVisible();
});

test("an agent can be messaged too, but not yourself", async ({ page }) => {
  await page.evaluate(() =>
    window.fake.receiveMessage(window.fake.addMember("Claude", { kind: "bot" }), "general", "Done"),
  );
  await nameButton(page, "Claude").click();
  await expect(card(page, "Claude").getByRole("button", { name: "Message" })).toBeVisible();
  await page.keyboard.press("Escape");

  await page.getByLabel("Message #general").fill("Me here");
  await page.getByLabel("Message #general").press("Enter");
  await nameButton(page, "Ada").click();
  await expect(card(page, "Ada").getByRole("button", { name: "Message" })).toHaveCount(0);
});

test("view in members opens the member list in settings", async ({ page }) => {
  await nameButton(page, "Grace").click();
  await card(page, "Grace").getByRole("button", { name: "View in members" }).click();

  const settings = page.getByRole("dialog", { name: "Settings" });
  await expect(settings.getByRole("list", { name: "Members" })).toContainText("Grace");
});

test("your own card shows you as offline while you appear offline", async ({ page }) => {
  await page.getByRole("button", { name: /^Ada\b/ }).click();
  await page.getByRole("menuitem", { name: "Appear offline" }).click();
  const composer = page.getByLabel("Message #general");
  await composer.fill("Quietly here");
  await composer.press("Enter");

  await nameButton(page, "Ada").click();
  const ada = card(page, "Ada");
  await expect(ada).toContainText("Not reachable right now");
  await expect(ada.getByRole("button", { name: "Mention" })).toHaveCount(0);
});

test("the caret is right after a mention from a card at once", async ({ page }) => {
  await page.getByLabel("Message #general").fill("hi");
  await nameButton(page, "Grace").click();
  const mention = card(page, "Grace").getByRole("button", { name: "Mention" });
  await expect(mention).toBeVisible();

  const result = await mention.evaluate((element: HTMLElement) => {
    element.click();
    const box = document.activeElement;
    return box instanceof HTMLTextAreaElement ? [box.value, box.selectionStart] : null;
  });
  expect(result).toEqual(["hi @Grace ", 10]);
});

const outlineOf = (element: HTMLElement | SVGElement) => getComputedStyle(element).outlineStyle;

test("a card opened with the mouse shows no focus ring; from the keyboard it does", async ({ page }) => {
  const composer = page.getByLabel("Message #general");
  await composer.fill("Mine");
  await composer.press("Enter");
  await nameButton(page, "Ada").click();
  // Focus goes into the card itself, so no button shows a ring however the engine judges :focus-visible.
  await expect(card(page, "Ada")).toBeFocused();
  expect(await card(page, "Ada").evaluate(outlineOf)).toBe("none");
  await page.keyboard.press("Tab");
  await expect(card(page, "Ada").getByRole("button", { name: "View in members" })).toBeFocused();
  await page.keyboard.press("Escape");
  await expect(card(page, "Ada")).toHaveCount(0);
  await nameButton(page, "Ada").click();
  await page.mouse.click(5, 5);
  await expect(card(page, "Ada")).toHaveCount(0);

  await nameButton(page, "Grace").focus();
  await page.keyboard.press("Enter");
  const message = card(page, "Grace").getByRole("button", { name: "Message" });
  await expect(message).toBeFocused();
  expect(await message.evaluate(outlineOf)).not.toBe("none");
});
