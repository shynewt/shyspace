import { expect, type Page, test } from "@playwright/test";
import { answerWith, failWith } from "./support";

declare global {
  interface Window {
    finishStorageSave?: () => void;
  }
}

async function openSettings(page: Page) {
  await page.goto("");
  await expect(page.getByLabel("Message #general")).toBeVisible();
  await page.keyboard.press("Control+,");
  return page.getByRole("dialog", { name: "Settings" });
}

test("you can change the name everyone sees", async ({ page }) => {
  const settings = await openSettings(page);
  await settings.getByLabel("Display name").fill("Ada Lovelace");
  await settings.getByRole("button", { name: "Save profile" }).click();

  await expect(page.getByText("Your profile is updated for everyone.")).toBeVisible();
  expect(await page.evaluate(() => window.fake.handle("members", undefined))).toContainEqual(
    expect.objectContaining({ name: "Ada Lovelace", is_admin: true }),
  );
});

test("the theme can be chosen and kept", async ({ page }) => {
  const settings = await openSettings(page);
  await settings.getByRole("tab", { name: "Appearance" }).click();
  await settings.getByRole("radio", { name: "Dark" }).click();
  await expect(page.locator("html")).toHaveAttribute("data-theme", "dark");

  await page.reload();
  await expect(page.locator("html")).toHaveAttribute("data-theme", "dark");
});

test("notification text can be hidden", async ({ page }) => {
  const settings = await openSettings(page);
  await settings.getByRole("tab", { name: "Notifications" }).click();
  await settings.getByRole("radio", { name: "Hide text" }).click();
  await page.keyboard.press("Escape");
  await page.getByRole("button", { name: "New channel" }).click();
  await page.getByLabel("Channel name").fill("random");
  await page.getByLabel("Channel name").press("Enter");
  await expect(page.getByLabel("Message #random")).toBeVisible();
  await page.getByRole("button", { name: "general", exact: true }).click();

  await page.evaluate(() => window.fake.receiveMessage(window.fake.addMember("Grace"), "random", "secret plans"));
  await expect
    .poll(() => page.evaluate(() => window.fake.alertsShown))
    .toEqual([{ title: "Grace in #random", body: "New message" }]);
});

test("the accent colour can be chosen, kept and set back", async ({ page }) => {
  const settings = await openSettings(page);
  await settings.getByRole("tab", { name: "Appearance" }).click();
  const accents = settings.getByRole("radiogroup", { name: "Accent colour" });
  expect(
    await accents.getByRole("radio").evaluateAll((radios) => radios.map((radio) => radio.getAttribute("aria-label"))),
  ).toEqual(["Violet", "Blue", "Indigo", "Teal", "Pink", "Graphite"]);
  await expect(accents.getByRole("radio", { name: "Violet" })).toBeChecked();
  const swatchColors = await accents
    .locator("[data-accent]")
    .evaluateAll((swatches) => swatches.map((swatch) => getComputedStyle(swatch).backgroundColor));
  expect(new Set(swatchColors).size, "each swatch shows its own colour").toBe(6);
  const violet = await page.evaluate(() => getComputedStyle(document.documentElement).getPropertyValue("--accent"));

  await accents.getByRole("radio", { name: "Teal" }).click();
  await expect(page.locator("html")).toHaveAttribute("data-accent", "teal");
  await expect
    .poll(() => page.evaluate(() => getComputedStyle(document.documentElement).getPropertyValue("--accent")))
    .not.toBe(violet);

  await page.reload();
  await expect(page.locator("html")).toHaveAttribute("data-accent", "teal");
  await openSettings(page);
  await page.keyboard.press("Control+,");
  await settings.getByRole("tab", { name: "Appearance" }).click();
  await settings.getByRole("radiogroup", { name: "Light or dark" }).getByRole("radio", { checked: true }).focus();
  await page.keyboard.press("Tab");
  await expect(accents.getByRole("radio", { name: "Teal" })).toBeFocused();
  await expect(accents.getByRole("radio", { name: "Teal" })).toHaveCSS("outline-style", "solid");
  await page.keyboard.press("ArrowRight");
  await expect(accents.getByRole("radio", { name: "Pink" })).toBeChecked();
  await expect(page.locator("html")).toHaveAttribute("data-accent", "pink");

  await accents.getByRole("radio", { name: "Violet" }).click();
  await expect(page.locator("html")).not.toHaveAttribute("data-accent");
});

test("text on accent fills follows the accent", async ({ page }) => {
  await page.emulateMedia({ colorScheme: "dark" });
  const settings = await openSettings(page);
  await settings.getByLabel("Display name").fill("Ada Lovelace");
  const save = settings.getByRole("button", { name: "Save profile" });
  await expect(save).toHaveCSS("color", "rgb(255, 255, 255)");

  await settings.getByRole("tab", { name: "Appearance" }).click();
  await settings.getByRole("radio", { name: "Graphite" }).click();
  await settings.getByRole("tab", { name: "Profile" }).click();
  await settings.getByLabel("Display name").fill("Ada Lovelace");
  await expect(save).toHaveCSS("color", "rgb(27, 27, 25)");
});

test("the arrow keys move between choices", async ({ page }) => {
  const settings = await openSettings(page);
  await settings.getByRole("tab", { name: "Appearance" }).click();
  const theme = settings.getByRole("radiogroup", { name: "Light or dark" });
  await theme.getByRole("radio", { name: "System" }).focus();

  await page.keyboard.press("ArrowRight");
  await expect(theme.getByRole("radio", { name: "Light" })).toBeChecked();
  await expect(theme.getByRole("radio", { name: "Light" })).toBeFocused();
  await expect(page.locator("html")).toHaveAttribute("data-theme", "light");
});

test("settings fit the smallest window and scroll inside it", async ({ page }) => {
  await page.setViewportSize({ width: 640, height: 560 });
  const settings = await openSettings(page);
  await expect(settings).toBeInViewport({ ratio: 1 });
  await page.evaluate(() => {
    for (let index = 1; index <= 12; index++) window.fake.addMember(`Member ${index}`);
  });

  await settings.getByRole("tab", { name: "Members" }).first().click();
  const lastMember = settings.getByRole("listitem").filter({ hasText: "Member 12" });
  await lastMember.scrollIntoViewIfNeeded();
  await expect(lastMember).toBeInViewport({ ratio: 1 });
  await expect(settings).toBeInViewport({ ratio: 1 });
});

test("the close button closes settings", async ({ page }) => {
  const settings = await openSettings(page);
  await settings.getByRole("button", { name: "Close" }).click();
  await expect(settings).toBeHidden();
});

test("settings group your sections and the workspace's", async ({ page }) => {
  const settings = await openSettings(page);
  const sections = settings.getByRole("tablist", { name: "Settings sections" });
  await expect(sections.getByRole("tab")).toHaveText([
    "Profile",
    "Appearance",
    "Notifications",
    "Huddles",
    "Privacy & security",
    "Workspace",
    "Members",
    "Agents",
    "Storage",
    "Recovery",
  ]);
  await expect(sections.getByText("You", { exact: true })).toBeVisible();
  await expect(sections.getByText("Acme", { exact: true })).toBeVisible();
});

test("the arrow keys move between sections", async ({ page }) => {
  const settings = await openSettings(page);
  await expect(settings.getByRole("tab", { name: "Profile" })).toBeFocused();

  await page.keyboard.press("ArrowDown");
  await expect(settings.getByRole("tab", { name: "Appearance" })).toHaveAttribute("aria-selected", "true");
  await expect(settings.getByRole("radiogroup", { name: "Theme" })).toBeVisible();
});

test("settings keep their size from section to section", async ({ page }) => {
  await page.emulateMedia({ reducedMotion: "reduce" });
  const settings = await openSettings(page);
  const profileBox = await settings.boundingBox();

  await settings.getByRole("tab", { name: "Appearance" }).click();
  await expect(settings.getByRole("radiogroup", { name: "Theme" })).toBeVisible();
  expect(await settings.boundingBox()).toEqual(profileBox);
});

test("the palette opens settings at a section", async ({ page }) => {
  await page.goto("");
  await expect(page.getByLabel("Message #general")).toBeVisible();
  await page.keyboard.press("Control+k");
  await page.getByLabel("Search messages and pages").fill("notifications");
  await page.getByRole("option", { name: "Settings: Notifications" }).click();

  const settings = page.getByRole("dialog", { name: "Settings" });
  await expect(settings.getByRole("tab", { name: "Notifications" })).toHaveAttribute("aria-selected", "true");
  await expect(settings.getByRole("tab", { name: "Notifications" })).toBeFocused();
});

test("privacy explains the encryption, with details on request", async ({ page }) => {
  const settings = await openSettings(page);
  await settings.getByRole("tab", { name: "Privacy & security" }).click();
  await expect(settings.getByText(/encrypted on this device/)).toBeVisible();
  const details = settings.getByText(/system's keychain/);
  await expect(details).toBeHidden();

  await settings.getByText("How it works").click();
  await expect(details).toBeVisible();
});

test("a failed profile save keeps the new name and says why", async ({ page }) => {
  const settings = await openSettings(page);
  await failWith(page, "set_profile", "the workspace is offline");
  await settings.getByLabel("Display name").fill("Ada Lovelace");
  await settings.getByRole("button", { name: "Save profile" }).click();

  await expect(settings.getByRole("alert")).toHaveText("the workspace is offline");
  await expect(settings.getByLabel("Display name")).toHaveValue("Ada Lovelace");
});

test("the admin can change the storage keys", async ({ page }) => {
  const settings = await openSettings(page);
  await settings.getByRole("tab", { name: "Storage" }).click();
  await expect(settings.getByText("https://s3.example.com")).toBeVisible();
  await settings.getByLabel("Access key").fill("AKIDNEW");
  await settings.getByLabel("Secret key").fill("new-secret");
  await settings.getByRole("button", { name: "Save storage settings" }).click();

  await expect(page.getByRole("dialog", { name: "Save your updated recovery kit" })).toBeVisible();
  expect(await page.evaluate(() => window.fake.handle("storage_settings", undefined))).toMatchObject({
    access_key_id: "AKIDNEW",
  });
});

test("a failed storage save keeps what you typed and says why", async ({ page }) => {
  const settings = await openSettings(page);
  await settings.getByRole("tab", { name: "Storage" }).click();
  await failWith(page, "update_storage", "the bucket refused these keys");
  await settings.getByLabel("Access key").fill("AKIDNEW");
  await settings.getByLabel("Secret key").fill("wrong-secret");
  await settings.getByRole("button", { name: "Save storage settings" }).click();

  await expect(settings.getByRole("alert")).toHaveText("the bucket refused these keys");
  await expect(settings.getByLabel("Access key")).toHaveValue("AKIDNEW");
  await expect(settings.getByLabel("Secret key")).toHaveValue("wrong-secret");
});

/** Settings › Recovery, with the core reporting a recovery snapshot of `bytes`. */
async function recoveryWithSnapshot(page: Page, bytes: number) {
  const settings = await openSettings(page);
  const storage = await page.evaluate(() => window.fake.handle("storage_settings", undefined));
  await answerWith(
    page,
    "storage_settings",
    { ...(storage as object), recovery_snapshot_bytes: bytes },
    { kind: "storage" },
  );
  await settings.getByRole("tab", { name: "Recovery" }).click();
  return settings;
}

test("a recovery kit far from its size limit says nothing about it", async ({ page }) => {
  const settings = await recoveryWithSnapshot(page, 620_000);
  await expect(settings.getByRole("button", { name: "Show recovery kit" })).toBeVisible();
  await expect(settings.getByRole("tabpanel")).not.toContainText("32 MB");
});

test("a recovery kit nearing its size limit says so calmly, and still shows", async ({ page }) => {
  const settings = await recoveryWithSnapshot(page, 26 * 1024 * 1024);
  await expect(settings.getByRole("tabpanel")).toContainText("Your recovery kit is nearly too big");
  // No advice to remove people: that would only make the snapshot bigger.
  await expect(settings.getByText(/^It grows as your team grows/)).toHaveText(
    "It grows as your team grows, and is now 26 MB of the 32 MB it can hold. Past that, it can no longer restore your admin role. An update to lift this limit is planned.",
  );
  await expect(settings.getByRole("button", { name: "Show recovery kit" })).toBeVisible();
});

test("a recovery kit past its size limit says it can't restore, and offers no kit", async ({ page }) => {
  const settings = await recoveryWithSnapshot(page, 33 * 1024 * 1024);
  await expect(settings.getByRole("tabpanel")).toContainText("Your recovery kit can't restore this workspace");
  await expect(settings.getByRole("tabpanel")).toContainText("keep this device safe");
  await expect(settings.getByRole("button", { name: "Show recovery kit" })).toHaveCount(0);
});

test("the admin can show the recovery kit", async ({ page }) => {
  const settings = await openSettings(page);
  await settings.getByRole("tab", { name: "Recovery" }).click();
  await settings.getByRole("button", { name: "Show recovery kit" }).click();

  await expect(page.getByRole("dialog", { name: "Your recovery kit" })).toContainText(/shyrec1_/);
});

test("a recovery kit that can't restore the workspace says why, instead of showing a kit", async ({ page }) => {
  const settings = await openSettings(page);
  const tooLarge =
    "the recovery snapshot is too large for restore to read (32 MB at most), so a recovery kit can't restore this workspace";
  await failWith(page, "recovery_kit", tooLarge);
  await settings.getByRole("tab", { name: "Recovery" }).click();
  await settings.getByRole("button", { name: "Show recovery kit" }).click();

  await expect(settings.getByRole("alert")).toHaveText(tooLarge);
  await expect(page.getByRole("dialog", { name: "Your recovery kit" })).toHaveCount(0);
});

test("members see who manages storage and recovery", async ({ page }) => {
  await page.goto("?start=member");
  await expect(page.getByLabel("Message #general")).toBeVisible();
  await page.keyboard.press("Control+,");
  const settings = page.getByRole("dialog", { name: "Settings" });

  await settings.getByRole("tab", { name: "Storage" }).click();
  await expect(settings.getByRole("tabpanel")).toContainText("Managed by Ada.");
  await expect(settings.getByLabel("Secret key")).toHaveCount(0);
  await settings.getByRole("tab", { name: "Recovery" }).click();
  await expect(settings.getByRole("tabpanel")).toContainText("Managed by Ada.");
  await expect(settings.getByRole("button", { name: "Show recovery kit" })).toHaveCount(0);
});

/** The cheat sheet's transform in each frame while it opens: "none" throughout when motion is reduced. */
async function transformsWhileOpeningADialog(page: Page): Promise<string[]> {
  return page.evaluate(async () => {
    const seen = new Set<string>();
    const sampled = new Promise<void>((resolve) => {
      const start = performance.now();
      const sample = () => {
        const dialog = document.querySelector('[role="dialog"]');
        if (dialog) seen.add(getComputedStyle(dialog).transform);
        if (performance.now() - start < 300) requestAnimationFrame(sample);
        else resolve();
      };
      requestAnimationFrame(sample);
    });
    window.dispatchEvent(new KeyboardEvent("keydown", { key: "/", ctrlKey: true }));
    await sampled;
    return [...seen];
  });
}

test("reduce motion can be turned on whatever the system says", async ({ page }) => {
  const settings = await openSettings(page);
  await settings.getByRole("tab", { name: "Appearance" }).click();
  await settings.getByRole("radiogroup", { name: "Reduce motion" }).getByRole("radio", { name: "On" }).click();
  await page.keyboard.press("Escape");
  await expect(settings).toBeHidden();

  expect(await transformsWhileOpeningADialog(page)).toEqual(["none"]);
});

test("reduce motion can be turned off when the system asks for less", async ({ page }) => {
  await page.emulateMedia({ reducedMotion: "reduce" });
  const settings = await openSettings(page);
  await settings.getByRole("tab", { name: "Appearance" }).click();
  await settings.getByRole("radiogroup", { name: "Reduce motion" }).getByRole("radio", { name: "Off" }).click();
  await page.keyboard.press("Escape");
  await expect(settings).toBeHidden();

  const transforms = await transformsWhileOpeningADialog(page);
  expect(transforms.some((transform) => transform.startsWith("matrix("))).toBe(true);
});

test("typed text waits while you look at another section", async ({ page }) => {
  const settings = await openSettings(page);
  await settings.getByLabel("Display name").fill("Ada Lovelace");
  await settings.getByRole("tab", { name: "Appearance" }).click();
  await settings.getByRole("tab", { name: "Profile" }).click();

  await expect(settings.getByLabel("Display name")).toHaveValue("Ada Lovelace");
});

test("a storage save that ends on another section still shows the new kit", async ({ page }) => {
  const settings = await openSettings(page);
  await settings.getByRole("tab", { name: "Storage" }).click();
  // The save waits until the test lets it finish, so it ends after the move to Appearance however slow the engine.
  await page.evaluate(() => {
    const answer = window.fake.handle.bind(window.fake);
    window.fake.handle = (command, args) =>
      command === "update_storage"
        ? new Promise((resolve) => {
            window.finishStorageSave = () => resolve(answer(command, args));
          })
        : answer(command, args);
  });
  await settings.getByLabel("Secret key").fill("new-secret");
  await settings.getByRole("button", { name: "Save storage settings" }).click();
  await expect.poll(() => page.evaluate(() => typeof window.finishStorageSave)).toBe("function");
  await settings.getByRole("tab", { name: "Appearance" }).click();
  await expect(settings.getByRole("tab", { name: "Appearance" })).toHaveAttribute("aria-selected", "true");
  await page.evaluate(() => window.finishStorageSave?.());

  await expect(page.getByRole("dialog", { name: "Save your updated recovery kit" })).toBeVisible();
});

test("opening settings with the mouse leaves no focus ring on its sections", async ({ page }) => {
  await page.goto("");
  await page.getByLabel("Message #general").click();
  await page.getByRole("button", { name: /^Ada\b/ }).click();
  await page.getByRole("menuitem", { name: "Settings" }).click();
  const profile = page.getByRole("dialog", { name: "Settings" }).getByRole("tab", { name: "Profile" });

  await expect(profile).toBeFocused();
  await expect(profile).toHaveCSS("outline-style", "none");
  await expect(profile).toHaveCSS("box-shadow", "none");
});

test("moving between sections with the keyboard shows where focus is", async ({ page }) => {
  const settings = await openSettings(page);
  await page.keyboard.press("ArrowDown");

  await expect(settings.getByRole("tab", { name: "Appearance" })).toBeFocused();
  await expect(settings.getByRole("tab", { name: "Appearance" })).toHaveCSS("outline-style", "solid");
});

test("no section makes the whole settings window scroll past its end", async ({ page }) => {
  const settings = await openSettings(page);
  const sections = settings.getByRole("tablist", { name: "Settings sections" }).getByRole("tab");
  for (const section of await sections.all()) {
    await section.click();
    const overflow = await settings.evaluate((dialog) => dialog.scrollHeight - dialog.clientHeight);
    expect(overflow, `${await section.textContent()} scrolls the window`).toBeLessThanOrEqual(0);
  }
});

test("cursors in pages: others' can be hidden and yours kept to yourself, and both are kept", async ({ page }) => {
  const settings = await openSettings(page);
  await settings.getByRole("tab", { name: "Appearance" }).click();
  await settings
    .getByRole("radiogroup", { name: "Others' cursors in pages" })
    .getByRole("radio", { name: "Hide" })
    .click();
  await settings.getByRole("tab", { name: "Privacy" }).click();
  await settings
    .getByRole("radiogroup", { name: "Your cursor in pages" })
    .getByRole("radio", { name: "Don't share" })
    .click();

  await page.reload();
  const stored = await page.evaluate(() => JSON.parse(localStorage.getItem("shyspace.preferences") ?? "{}"));
  expect(stored).toMatchObject({ showCursorsInPages: false, shareCursorInPages: false });
});
