import { expect, type Page, test } from "@playwright/test";
import { failWith } from "./support";

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

test("keyboard focus stays on the huddle and mute buttons while they wait", async ({ page }) => {
  await openApp(page);
  await page.evaluate(() => {
    window.fake.huddles.holdJoining = true;
  });
  const start = page.getByRole("button", { name: "Start a huddle" });
  await start.focus();
  await page.keyboard.press("Enter");
  const joining = page.getByRole("button", { name: "Joining…" });
  await expect(joining).toHaveAttribute("aria-disabled", "true");
  await expect(joining).toBeFocused();
  await page.keyboard.press("Enter");
  expect(await page.evaluate(() => window.fake.huddles.joinedMuted.length)).toBe(1);
  await page.evaluate(() => window.fake.huddles.connect());
  await expect(page.getByRole("button", { name: "Leave the huddle" })).toBeFocused();

  const mute = bar(page).getByRole("button", { name: "Mute", exact: true });
  await mute.focus();
  await page.keyboard.press("Enter");
  await expect(mute).toHaveAttribute("aria-pressed", "true");
  await page.evaluate(() => {
    window.fake.huddles.holdMicOpening = true;
  });
  await page.keyboard.press("Enter");
  await expect(mute).toHaveText("Unmuting…");
  await expect(mute).toHaveAttribute("aria-disabled", "true");
  await expect(mute).toBeFocused();
  await page.keyboard.press("Enter");
  await page.evaluate(() => window.fake.huddles.releaseMic());
  await expect(mute).toHaveText("Mute");
  await expect(mute).toBeFocused();
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

test("a device that stops working says so only once it lasts, so a quick swap shows nothing", async ({ page }) => {
  await page.clock.install();
  await openApp(page);
  await page.getByRole("button", { name: "Start a huddle" }).click();
  await expect(bar(page)).toContainText("Just you so far");

  // A chosen mic unplugged: the default takes over within a second.
  await page.evaluate(() => window.fake.huddles.mic("lost"));
  await page.clock.runFor(500);
  await page.evaluate(() => window.fake.huddles.mic("live"));
  await page.clock.runFor(3000);
  await expect(bar(page)).not.toContainText("isn't working");

  await page.evaluate(() => window.fake.huddles.mic("lost"));
  await page.clock.runFor(1500);
  await expect(bar(page)).not.toContainText("isn't working");
  await page.clock.runFor(1000);
  await expect(bar(page)).toContainText("Your microphone isn't working. Trying again…");
  await expect(announcement(page)).toHaveText("Your microphone isn't working. Trying again");
});

test("a microphone or speakers that stop working offer another, and the notice goes once they work", async ({
  page,
}) => {
  await openApp(page);
  await page.getByRole("button", { name: "Start a huddle" }).click();

  await page.evaluate(() => window.fake.huddles.mic("lost"));
  await bar(page).getByRole("button", { name: "Choose another microphone" }).click();
  await expect(page.getByRole("combobox", { name: "Microphone" })).toBeVisible();
  await page.keyboard.press("Escape");
  await page.evaluate(() => window.fake.huddles.mic("live"));
  await expect(bar(page)).not.toContainText("isn't working");

  await page.evaluate(() => window.fake.huddles.output("lost"));
  await expect(bar(page)).toContainText("Your speakers aren't working. Trying again…");
  await expect(announcement(page)).toHaveText("Your speakers aren't working. Trying again");
  await expect(bar(page).getByRole("button", { name: "Choose other speakers" })).toBeVisible();
  await page.evaluate(() => window.fake.huddles.output("playing"));
  await expect(bar(page)).not.toContainText("aren't working");
});

test("with no device marked as the default, the pickers still offer the system's default", async ({ page }) => {
  await openApp(page);
  // PipeWire marks none: following the default is choosing no device.
  await page.evaluate(() =>
    window.fake.huddles.setDevices({
      inputs: [
        { id: "analog-mic", name: "Analog Input", is_default: false, is_bluetooth: false },
        { id: "usb-mic", name: "USB Microphone", is_default: false, is_bluetooth: false },
      ],
      outputs: [{ id: "analog-out", name: "Analog Output", is_default: false, is_bluetooth: false }],
    }),
  );
  await page.keyboard.press("Control+,");
  const settings = page.getByRole("dialog", { name: "Settings" });
  await settings.getByRole("tab", { name: "Huddles" }).click();

  const microphone = settings.getByRole("combobox", { name: "Microphone" });
  await expect(microphone).toHaveText("System default");
  await expect(settings.getByRole("combobox", { name: "Speakers" })).toHaveText("System default");
  await microphone.click();
  await page.getByRole("option", { name: "USB Microphone" }).click();
  await expect(microphone).toHaveText("USB Microphone");
  expect(await page.evaluate(() => window.fake.huddles.chosenDevices)).toEqual({ input: "usb-mic", output: null });

  // Unplugged, the choice stays (it comes back when the device does), and says what's used meanwhile.
  await page.evaluate(() =>
    window.fake.huddles.setDevices({
      inputs: [{ id: "analog-mic", name: "Analog Input", is_default: false, is_bluetooth: false }],
      outputs: [{ id: "analog-out", name: "Analog Output", is_default: false, is_bluetooth: false }],
    }),
  );
  await expect(microphone).toHaveText("Not connected, using the system default");

  await microphone.click();
  await page.getByRole("option", { name: "System default", exact: true }).click();
  await expect(microphone).toHaveText("System default");
  expect(await page.evaluate(() => window.fake.huddles.chosenDevices)).toEqual({ input: null, output: null });
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

/** Grace starts a huddle in your DM with her, which rings; returns the DM's id and hers. */
async function graceCalls(page: Page): Promise<{ dm: string; grace: string }> {
  return page.evaluate(() => {
    const grace = window.fake.addMember("Grace");
    const dm = window.fake.handle("open_dm", { members: [grace] }) as string;
    window.fake.huddles.join(grace, dm);
    return { dm, grace };
  });
}

function incoming(page: Page) {
  return page.getByRole("region", { name: "Incoming huddle" });
}

test("an incoming DM huddle shows a card while it rings, and Join answers it", async ({ page }) => {
  await openApp(page);
  await graceCalls(page);

  await expect(incoming(page)).toContainText("Grace started a huddle");
  await expect(announcement(page)).toHaveText("Grace started a huddle");
  await workspaceNav(page).getByRole("button", { name: "Inbox" }).click();
  await incoming(page).getByRole("button", { name: "Join" }).click();

  await expect(incoming(page)).toHaveCount(0);
  await expect(bar(page)).toBeVisible();
  expect(await page.evaluate(() => window.fake.huddles.ringing)).toBeNull();
});

test("a window that opens while a huddle rings shows the card", async ({ page }) => {
  await page.addInitScript(() => {
    window.prepareFake = (fake) => {
      const grace = fake.addMember("Grace");
      fake.huddles.join(grace, fake.handle("open_dm", { members: [grace] }) as string);
    };
  });
  await openApp(page);
  await expect(incoming(page)).toContainText("Grace started a huddle");
});

test("Decline stops the ring without joining, and the huddle stays there to join later", async ({ page }) => {
  await openApp(page);
  await graceCalls(page);

  await incoming(page).getByRole("button", { name: "Decline" }).click();
  await expect(incoming(page)).toHaveCount(0);
  await expect(bar(page)).toHaveCount(0);
  expect(await page.evaluate(() => window.fake.huddles.ringing)).toBeNull();

  await workspaceNav(page)
    .getByRole("button", { name: /^Grace, direct message/ })
    .click();
  await expect(page.getByRole("button", { name: "Join Grace" })).toBeVisible();
});

test("the card goes when the caller hangs up, and nothing rings while you're in a huddle", async ({ page }) => {
  await openApp(page);
  const { dm, grace } = await graceCalls(page);
  await expect(incoming(page)).toBeVisible();
  await page.evaluate(([channel, member]) => window.fake.huddles.leave(member, channel), [dm, grace]);
  await expect(incoming(page)).toHaveCount(0);

  await page.getByRole("button", { name: "Start a huddle" }).click();
  await expect(bar(page)).toBeVisible();
  await page.evaluate(([channel, member]) => window.fake.huddles.join(member, channel), [dm, grace]);
  await expect(
    workspaceNav(page).getByRole("button", { name: /^Grace, direct message, huddle with Grace/ }),
  ).toBeVisible();
  await expect(incoming(page)).toHaveCount(0);
});

test("with the sidebar hidden, an incoming huddle rings in the header", async ({ page }) => {
  await openApp(page);
  await page.keyboard.press("Control+\\");
  await graceCalls(page);

  const pill = page.getByRole("group", { name: "Incoming huddle" });
  await expect(pill).toContainText("Grace");
  await pill.getByRole("button", { name: "Join Grace's huddle" }).click();
  await expect(page.getByRole("group", { name: "Your huddle" })).toBeVisible();
  await expect(pill).toHaveCount(0);
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
  await expect(microphone).toHaveText("System default (Built-in Microphone)");
  await microphone.click();
  await page.getByRole("option", { name: "Headset" }).click();
  await expect(microphone).toHaveText("Headset");
  await expect(settings).toContainText("A headset's microphone lowers what you hear to call quality.");
  expect(await page.evaluate(() => window.fake.huddles.chosenDevices)).toEqual({ input: "headset-mic", output: null });

  const sounds = settings.getByRole("switch", { name: "Play huddle sounds" });
  await expect(sounds).toBeChecked();
  await sounds.click();
  await expect(sounds).not.toBeChecked();
  expect(await page.evaluate(() => window.fake.huddles.soundsOn)).toBe(false);

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

test("without voice in this build, Settings › Huddles says why and offers nothing that can't work", async ({
  page,
}) => {
  await openApp(page);
  await failWith(page, "huddle_devices", "this build has no audio");
  await page.keyboard.press("Control+,");
  const settings = page.getByRole("dialog", { name: "Settings" });
  await settings.getByRole("tab", { name: "Huddles" }).click();

  await expect(settings.getByText("Voice isn't available")).toBeVisible();
  await expect(settings).toContainText("this build has no audio");
  await expect(settings.getByRole("switch")).toHaveCount(0);
  await expect(page.getByRole("alert")).toHaveCount(0);
});
