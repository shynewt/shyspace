import { expect, type Page, test } from "@playwright/test";

function workspaceNav(page: Page) {
  return page.getByRole("navigation", { name: "Workspace" });
}

function bar(page: Page) {
  return page.getByRole("region", { name: "Your huddle" });
}

/** What the screen reader was last told about your huddle. */
function announcement(page: Page) {
  return page.getByRole("status", { name: "Huddle news" });
}

async function openApp(page: Page) {
  await page.goto("");
  await expect(page.getByLabel("Message #general")).toBeVisible();
}

/** Grace (and anyone else named) joins #general's huddle; returns their ids in order. */
async function othersJoin(page: Page, names: string[]): Promise<string[]> {
  return page.evaluate((people) => {
    const general = window.fake.handle("channels", undefined) as Array<{ id: string; name: string }>;
    const channel = general.find((candidate) => candidate.name === "general");
    if (!channel) throw new Error("no #general");
    return people.map((name) => {
      const id = window.fake.addMember(name);
      window.fake.huddles.join(id, channel.id);
      return id;
    });
  }, names);
}

async function generalId(page: Page): Promise<string> {
  return page.evaluate(() => {
    const channels = window.fake.handle("channels", undefined) as Array<{ id: string; name: string }>;
    const general = channels.find((candidate) => candidate.name === "general");
    if (!general) throw new Error("no #general");
    return general.id;
  });
}

test("one click starts a huddle, the bar shows it on every view, and Leave ends it", async ({ page }) => {
  await openApp(page);
  await page.getByRole("button", { name: "Start a huddle" }).click();

  await expect(bar(page)).toContainText("Just you so far");
  await expect(page.getByRole("button", { name: "Leave the huddle" })).toBeVisible();
  await expect(workspaceNav(page).getByRole("button", { name: /^general, huddle with You/ })).toBeVisible();
  await expect(announcement(page)).toHaveText("You joined the huddle in #general");

  await workspaceNav(page).getByRole("button", { name: "Inbox" }).click();
  await expect(bar(page)).toBeVisible();

  await bar(page).getByRole("button", { name: "Leave" }).click();
  await expect(bar(page)).toHaveCount(0);
  await expect(announcement(page)).toHaveText("You left the huddle");
});

test("before joining, the header and sidebar show who's in without ringing", async ({ page }) => {
  await openApp(page);
  await othersJoin(page, ["Grace", "Linus"]);

  await expect(page.getByRole("button", { name: "Join Grace and Linus" })).toBeVisible();
  await expect(workspaceNav(page).getByRole("button", { name: /^general, huddle with Grace and Linus/ })).toBeVisible();
  expect(await page.evaluate(() => window.fake.alertsShown.length)).toBe(0);
});

test("joining shows a brief spinner, then everyone connecting, then the call", async ({ page }) => {
  await openApp(page);
  const [grace] = await othersJoin(page, ["Grace"]);
  await page.evaluate(() => {
    window.fake.huddles.holdJoining = true;
  });

  await page.getByRole("button", { name: "Join Grace" }).click();
  await expect(page.getByText("Joining…")).toBeVisible();
  await page.evaluate(() => window.fake.huddles.connect());
  await expect(bar(page)).toContainText("Connecting…");

  await page.evaluate((id) => window.fake.huddles.link(id, "direct"), grace);
  await expect(bar(page)).not.toContainText("Connecting…");
  await expect(bar(page).getByRole("button", { name: "In the huddle: You and Grace. Show details" })).toBeVisible();
});

test("a full huddle says so and can't be joined", async ({ page }) => {
  await openApp(page);
  const general = await generalId(page);
  await page.evaluate((channel) => {
    for (const name of ["Grace", "Linus", "Mia", "Sam", "Noor", "Theo", "Ivy", "Rosa"]) window.fake.addMember(name);
    window.fake.huddles.fill(channel);
  }, general);

  const full = page.getByRole("button", { name: /^Huddle full: / });
  await expect(full).toHaveAttribute("aria-disabled", "true");
  // It has no action; a forced click proves that, since Playwright won't click a disabled-looking button.
  await full.click({ force: true });
  expect(await page.evaluate(() => window.fake.huddles.joinedMuted.length)).toBe(0);
  await expect(workspaceNav(page).getByRole("button", { name: /^general, huddle with .*, full/ })).toBeVisible();
});

test("mute says its state in words, unmuting waits for the microphone, and the keys work", async ({ page }) => {
  await openApp(page);
  await page.getByRole("button", { name: "Start a huddle" }).click();
  const mute = bar(page).getByRole("button", { name: "Mute", exact: true });

  await mute.click();
  await expect(mute).toHaveAttribute("aria-pressed", "true");
  await expect(mute).toHaveText("Unmute");
  await expect(bar(page)).toContainText("Muted");
  await expect(announcement(page)).toHaveText("Muted");

  await page.evaluate(() => {
    window.fake.huddles.holdMicOpening = true;
  });
  await mute.click();
  await expect(mute).toHaveText("Unmuting…");
  await expect(mute).toBeDisabled();
  await page.evaluate(() => window.fake.huddles.releaseMic());
  await expect(mute).toHaveText("Mute");
  await expect(mute).toHaveAttribute("aria-pressed", "false");

  await page.evaluate(() => {
    window.fake.huddles.holdMicOpening = false;
  });
  await page.keyboard.press("Control+Shift+Space");
  await expect(mute).toHaveAttribute("aria-pressed", "true");
  await page.keyboard.press("Control+Shift+H");
  await expect(bar(page)).toHaveCount(0);
  await page.keyboard.press("Control+Shift+H");
  await expect(bar(page)).toBeVisible();
});

test("the details show who's speaking and how each person is connected", async ({ page }) => {
  await openApp(page);
  const [grace, linus, mia] = await othersJoin(page, ["Grace", "Linus", "Mia"]);
  await page.getByRole("button", { name: "Join Grace, Linus and Mia" }).click();
  await page.evaluate(
    ([graceId, linusId, miaId]) => {
      window.fake.huddles.link(graceId, "direct");
      window.fake.huddles.link(linusId, "relayed");
      window.fake.huddles.link(miaId, "direct");
      window.fake.huddles.needsUpdate(miaId, true);
      window.fake.huddles.speaking(graceId, true);
    },
    [grace, linus, mia],
  );

  await bar(page).getByRole("button", { name: "Huddle details" }).click();
  const people = page.getByRole("list", { name: "In the huddle" });
  await expect(people.getByRole("listitem").filter({ hasText: "Grace" })).toContainText("speaking");
  await expect(people.getByRole("listitem").filter({ hasText: "Linus" })).toContainText("relayed: may sound worse");
  await expect(people.getByRole("listitem").filter({ hasText: "Mia" })).toContainText(
    "needs an update to talk with you",
  );
  await expect(people.getByRole("listitem").filter({ hasText: /^.?You/ })).not.toContainText("speaking");
});

test("a reconnect shows only once it lasts past the grace", async ({ page }) => {
  await page.clock.install();
  await openApp(page);
  const [grace] = await othersJoin(page, ["Grace"]);
  await page.getByRole("button", { name: "Join Grace" }).click();
  await page.evaluate((id) => window.fake.huddles.link(id, "direct"), grace);
  await expect(bar(page)).not.toContainText("Connecting…");

  await page.evaluate((id) => window.fake.huddles.link(id, "reconnecting"), grace);
  await page.clock.runFor(2000);
  await expect(bar(page)).not.toContainText("Reconnecting…");
  await page.clock.runFor(1500);
  await expect(bar(page)).toContainText("Reconnecting…");
  await expect(announcement(page)).toHaveText("Reconnecting");

  await page.evaluate((id) => window.fake.huddles.link(id, "direct"), grace);
  await expect(bar(page)).not.toContainText("Reconnecting…");
  await expect(announcement(page)).toHaveText("Reconnected");
});

test("microphone and headset trouble each show one calm line with its fix", async ({ page }) => {
  await openApp(page);
  await page.getByRole("button", { name: "Start a huddle" }).click();

  await page.evaluate(() => window.fake.huddles.mic("silent"));
  await expect(bar(page)).toContainText("Your microphone sends no sound.");
  await bar(page).getByRole("button", { name: "Choose another microphone" }).click();
  await expect(page.getByRole("combobox", { name: "Microphone" })).toBeVisible();
  await page.keyboard.press("Escape");

  await page.evaluate(() => window.fake.huddles.mic("permission_denied"));
  await expect(bar(page)).toContainText("shyspace can't use your microphone.");
  await expect(bar(page).getByRole("button", { name: "Mute", exact: true })).toBeDisabled();

  await page.evaluate(() => window.fake.huddles.mic("lost"));
  await expect(page.getByText("Your microphone was disconnected. Switching to the default.")).toBeVisible();

  await page.evaluate(() => {
    window.fake.handle("set_huddle_devices", { choice: { input: "headset-mic", output: "headset" } });
    window.fake.huddles.output("bluetooth_call_mode");
  });
  await expect(bar(page)).toContainText("Your headset switched to call quality.");
  await bar(page).getByRole("button", { name: "Use Built-in Microphone" }).click();
  await expect
    .poll(() => page.evaluate(() => window.fake.huddles.chosenDevices))
    .toEqual({ input: null, output: "headset" });
});

test("joining another huddle leaves this one, with an undo", async ({ page }) => {
  await openApp(page);
  await page.getByRole("button", { name: "Start a huddle" }).click();
  await expect(bar(page)).toContainText("general");
  await page.evaluate(() => window.fake.handle("create_channel", { name: "random" }));
  await workspaceNav(page).getByRole("button", { name: "random", exact: true }).click();

  await page.getByRole("button", { name: "Start a huddle" }).click();
  await expect(bar(page)).toContainText("random");
  await expect(page.getByText("Left the huddle in #general")).toBeVisible();
  await page.getByRole("button", { name: "Undo" }).click();
  await expect(bar(page)).toContainText("general");
});

test("someone starting a huddle in a DM rings once, and the ring opens the DM", async ({ page }) => {
  await openApp(page);
  await page.evaluate(() => {
    const grace = window.fake.addMember("Grace");
    const dm = window.fake.handle("open_dm", { members: [grace] }) as string;
    window.fake.huddles.join(grace, dm);
  });

  await expect
    .poll(() => page.evaluate(() => window.fake.alertsShown))
    .toEqual([{ title: "Grace", body: "Started a huddle" }]);
  await expect(
    workspaceNav(page).getByRole("button", { name: /^Grace, direct message, huddle with Grace/ }),
  ).toBeVisible();

  await page.evaluate(() => window.fake.clickAlert(0));
  await expect(page.getByRole("button", { name: "Join Grace" })).toBeVisible();
});

test("with the sidebar hidden, the huddle moves to a pill in the header", async ({ page }) => {
  await openApp(page);
  await page.getByRole("button", { name: "Start a huddle" }).click();
  await page.keyboard.press("Control+\\");

  const pill = page.getByRole("group", { name: "Your huddle" });
  await expect(pill).toBeVisible();
  await pill.getByRole("button", { name: "Mute" }).click();
  await expect(pill.getByRole("button", { name: "Unmute" })).toHaveAttribute("aria-pressed", "true");
  await pill.getByRole("button", { name: "Leave the huddle" }).click();
  await expect(pill).toHaveCount(0);
});

test("Settings picks the microphone and speakers, and can join muted", async ({ page }) => {
  await openApp(page);
  await page.keyboard.press("Control+,");
  const settings = page.getByRole("dialog", { name: "Settings" });
  await settings.getByRole("tab", { name: "Huddles" }).click();

  const microphone = settings.getByRole("combobox", { name: "Microphone" });
  await expect(microphone).toHaveText("Built-in Microphone (default)");
  await microphone.click();
  await page.getByRole("option", { name: "Headset" }).click();
  await expect(microphone).toHaveText("Headset");
  await expect(settings).toContainText("A headset's microphone lowers what you hear to call quality.");
  expect(await page.evaluate(() => window.fake.huddles.chosenDevices)).toEqual({ input: "headset-mic", output: null });

  await settings.getByRole("switch", { name: "Join huddles muted" }).click();
  await page.keyboard.press("Escape");
  await page.getByRole("button", { name: "Start a huddle" }).click();
  await expect(bar(page).getByRole("button", { name: "Mute", exact: true })).toHaveAttribute("aria-pressed", "true");
  expect(await page.evaluate(() => window.fake.huddles.joinedMuted)).toEqual([true]);
});

test("a join that fails says why, and notes have no huddle", async ({ page }) => {
  await openApp(page);
  await page.evaluate(() => {
    window.fake.huddles.failNextJoin = "this build of shyspace can't join huddles: it has no audio";
  });
  await page.getByRole("button", { name: "Start a huddle" }).click();
  await expect(page.getByText("this build of shyspace can't join huddles: it has no audio")).toBeVisible();
  await expect(bar(page)).toHaveCount(0);

  await workspaceNav(page)
    .getByRole("button", { name: /^Notes/ })
    .click();
  await expect(page.getByLabel("Write a note")).toBeVisible();
  await expect(page.getByRole("button", { name: "Start a huddle" })).toHaveCount(0);
});
