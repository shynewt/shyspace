import { expect, type Locator, test } from "@playwright/test";

/** A single red pixel. */
const PNG = [
  137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 1, 0, 0, 0, 1, 8, 6, 0, 0, 0, 31, 21, 196, 137,
  0, 0, 0, 13, 73, 68, 65, 84, 120, 156, 99, 248, 207, 192, 240, 31, 0, 5, 0, 1, 255, 137, 153, 61, 29, 0, 0, 0, 0, 73,
  69, 78, 68, 174, 66, 96, 130,
];

const hasPicture = (place: Locator) => expect(place.locator("img").first()).toBeVisible();

test("someone's picture shows wherever they appear, not their letter", async ({ page }) => {
  await page.goto("");
  await expect(page.getByLabel("Message #general")).toBeVisible();
  await page.evaluate((png) => {
    const grace = window.fake.addMember("Grace");
    window.fake.files.setPicture(grace, png);
    window.fake.receiveMessage(grace, "general", "Morning");
    const dm = window.fake.handle("open_dm", { members: [grace] });
    if (typeof dm !== "string") throw new Error("no DM");
    window.fake.receiveMessageIn(grace, dm, "Got a minute?");
  }, PNG);

  // A message in the channel.
  await hasPicture(page.getByRole("article").filter({ hasText: "Morning" }));
  // The sidebar's direct messages.
  await hasPicture(page.getByRole("navigation", { name: "Workspace" }).getByRole("button", { name: /^Grace/ }));
  // Suggestions while writing a mention.
  await page.getByLabel("Message #general").pressSequentially("@Gr");
  await hasPicture(
    page.getByRole("listbox", { name: "People and pages to mention" }).getByRole("option", { name: /Grace/ }),
  );
  await page.getByLabel("Message #general").fill("");
  // Their profile card.
  await page.getByRole("article").filter({ hasText: "Morning" }).getByRole("button", { name: "Grace" }).first().click();
  await hasPicture(page.getByRole("dialog", { name: /Grace/ }));
  await page.keyboard.press("Escape");
  // The member list.
  await page.keyboard.press("Control+,");
  const settings = page.getByRole("dialog", { name: "Settings" });
  await settings.getByRole("tab", { name: "Members" }).first().click();
  await hasPicture(settings.getByRole("list", { name: "Members" }).getByRole("listitem").filter({ hasText: "Grace" }));
});
