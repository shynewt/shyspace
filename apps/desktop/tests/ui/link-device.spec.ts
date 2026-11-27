import { expect, type Page, test } from "@playwright/test";

const LINK_CODE = `shyl1_${"ab".repeat(30)}`;

async function chooseLinking(page: Page) {
  await page.goto("?start=onboarding");
  await expect(page.getByRole("heading", { name: "Welcome to shyspace" })).toBeVisible();
  await page.getByRole("radio", { name: "I already use shyspace" }).click();
}

test("a code that isn't a link code is explained right away", async ({ page }) => {
  await chooseLinking(page);

  await page.getByLabel("Link code").fill("hello");

  await expect(page.getByText("This isn't a link code. Copy it again from your other device.")).toBeVisible();
});

test("pasting a link code shows the number to tap on the other device", async ({ page }) => {
  await chooseLinking(page);

  await page.getByLabel("Link code").fill(LINK_CODE);

  await expect(page.getByText("On your other device, tap this number")).toBeVisible();
  await expect(page.getByText("482, 917")).toBeAttached();
  await expect(page.getByText("There's nothing to tap here.")).toBeVisible();
});

test("after the hand-over the workspace opens for reading, and Send waits until it's added", async ({ page }) => {
  await chooseLinking(page);
  await page.getByLabel("Link code").fill(LINK_CODE);
  await expect(page.getByText("On your other device, tap this number")).toBeVisible();

  await page.evaluate(() => window.fake.devices.handOver());

  const composer = page.getByLabel("Message #general");
  await expect(composer).toBeVisible();
  await expect(page.getByText(/This device can read now\. It can send once your workspace adds it/)).toBeVisible();
  await composer.fill("hello from my laptop");
  await expect(page.getByRole("button", { name: "Send" })).toBeDisabled();

  await page.evaluate(() => window.fake.finishLinking());

  await expect(page.getByText(/This device can read now/)).toHaveCount(0);
  await expect(page.getByRole("button", { name: "Send" })).toBeEnabled();
});

test("cancelling while waiting goes back to the code, with nothing kept", async ({ page }) => {
  await chooseLinking(page);
  await page.getByLabel("Link code").fill(LINK_CODE);

  await page.getByRole("button", { name: "Cancel" }).click();

  await expect(page.getByLabel("Link code")).toBeVisible();
  await expect(page.getByText("You stopped linking this device")).toHaveCount(0);
});

test("the other device stopping the link says so", async ({ page }) => {
  await chooseLinking(page);
  await page.getByLabel("Link code").fill(LINK_CODE);
  await expect(page.getByText("On your other device, tap this number")).toBeVisible();

  await page.evaluate(() => window.fake.devices.stopFromOtherDevice());

  await expect(page.getByText("Your other device stopped the link. Nothing was shared.")).toBeVisible();
});

test("after a restart, a link waiting for its hand-over shows its number again and goes on", async ({ page }) => {
  await page.addInitScript(() => {
    window.prepareFake = (fake) => {
      fake.devices.interrupted = { code: "482 917" };
    };
  });
  await page.goto("?start=onboarding");

  await expect(page.getByText("On your other device, tap this number")).toBeVisible();
  await page.evaluate(() => window.fake.devices.handOver());
  await expect(page.getByLabel("Message #general")).toBeVisible();
});

test("a link that ends after the hand-over says why and leaves the workspace", async ({ page }) => {
  await chooseLinking(page);
  await page.getByLabel("Link code").fill(LINK_CODE);
  await expect(page.getByText("On your other device, tap this number")).toBeVisible();
  await page.evaluate(() => window.fake.devices.handOver());
  await expect(page.getByLabel("Message #general")).toBeVisible();

  await page.evaluate(() => window.fake.endLinking("Your other device was removed before this one was added."));

  await expect(page.getByText("Your other device was removed before this one was added.")).toBeVisible();
  await expect(page.getByRole("heading", { name: "Welcome to shyspace" })).toBeVisible();
});

test("a linked device offers no private messages yet, and says so calmly", async ({ page }) => {
  await chooseLinking(page);
  await page.getByLabel("Link code").fill(LINK_CODE);
  await expect(page.getByText("On your other device, tap this number")).toBeVisible();
  await page.evaluate(() => window.fake.devices.handOver());
  await expect(page.getByLabel("Message #general")).toBeVisible();

  const nav = page.getByRole("navigation", { name: "Workspace" });
  const dms = nav.getByRole("region", { name: "Direct messages" });
  await expect(dms.getByText("Private messages come to linked devices in a later update.")).toBeVisible();
  await expect(dms.getByRole("button", { name: "New direct message" })).toHaveCount(0);
  await expect(nav.getByText("Notes", { exact: true })).toHaveCount(0);

  const channels = nav.getByRole("region", { name: "Channels" });
  await channels.hover();
  await channels.getByRole("button", { name: "Actions for the Channels section" }).click();
  await expect(page.getByRole("menuitem", { name: "New section" })).toBeVisible();
  await expect(page.getByRole("menuitem", { name: "New private channel" })).toHaveCount(0);
  await page.keyboard.press("Escape");

  await page.evaluate(() => {
    const grace = window.fake.addMember("Grace");
    window.fake.receiveMessage(grace, "general", "Morning all");
  });
  await page
    .getByRole("log", { name: "Messages in #general" })
    .getByRole("button", { name: "Grace", exact: true })
    .click();
  const grace = page.getByRole("dialog", { name: "Grace's profile" });
  await expect(grace.getByRole("button", { name: "Mention" })).toBeVisible();
  await expect(grace.getByRole("button", { name: "Message" })).toHaveCount(0);
});
