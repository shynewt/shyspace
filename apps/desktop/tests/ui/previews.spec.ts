import { expect, test } from "@playwright/test";

const MARKDOWN = [
  "## Plan for **Friday**",
  "> quoted _idea_",
  "- ship `v2` [notes](https://example.com/notes)",
  "1. tell [@channel](shy://mention/channel)",
].join("\n");

test.beforeEach(async ({ page }) => {
  await page.goto("");
  await page.getByRole("navigation", { name: "Workspace" }).waitFor();
});

test("previews show markdown as plain words, one part after another", async ({ page }) => {
  await page.getByRole("button", { name: "Inbox", exact: true }).click();
  await page.evaluate(
    (text) =>
      window.fake.receiveMessage(
        window.fake.addMember("Grace"),
        "general",
        `[@Ada](shy://member/${window.fake.session?.me})\n${text}`,
      ),
    MARKDOWN,
  );

  await expect(page.getByRole("region", { name: "Mentions and replies" })).toContainText(
    "@Ada · Plan for Friday · quoted idea · ship v2 notes · tell @channel",
  );
});

test("search results show mentions and links as words", async ({ page }) => {
  await page.evaluate(() =>
    window.fake.receiveMessage(
      window.fake.addMember("Grace"),
      "general",
      "[@channel](shy://mention/channel) standup moved",
    ),
  );

  await page.keyboard.press("Control+k");
  await page.getByLabel("Search messages and pages").fill("channel standup");
  const hit = page.getByRole("option", { name: /standup moved/ });
  await expect(hit).toContainText("@channel standup moved");
  await expect(hit).not.toContainText("shy://");
  await expect(hit.locator("mark.hit")).toHaveText(["channel", "standup"]);
});

test("hostile message text can't stall the previews", async ({ page }) => {
  await page.getByRole("button", { name: "Inbox", exact: true }).click();
  await page.evaluate(() => {
    const mallory = window.fake.addMember("Mallory");
    const mention = `[@Ada](shy://member/${window.fake.session?.me}) `;
    for (let index = 0; index < 20; index += 1) {
      window.fake.receiveMessage(mallory, "general", mention + "*a ".repeat(13000));
    }
  });

  await expect(page.getByRole("region", { name: "Mentions and replies" }).getByRole("button")).toHaveCount(20, {
    timeout: 3000,
  });
});

test("a thread's latest reply and a search result show the core's plain text as it is", async ({ page }) => {
  await page.evaluate(() => {
    const grace = window.fake.addMember("Grace");
    const root = window.fake.receiveMessage(grace, "general", "Who checks the totals?");
    window.fake.receiveReply(grace, root, "*kept* as typed");
  });
  await page.getByRole("button", { name: "general", exact: true }).click();
  await expect(page.getByRole("button", { name: /1 reply/ })).toContainText("*kept* as typed");

  await page.keyboard.press("Control+k");
  await page.getByLabel("Search messages and pages").fill("kept");
  await expect(page.getByRole("option", { name: /as typed/ })).toContainText("*kept* as typed");
});
