import { expect, type Page, test } from "@playwright/test";

test.use({ locale: "en-US", timezoneId: "Europe/Lisbon" });

declare global {
  interface Window {
    detailsSaved?: unknown[];
  }
}

async function openProfile(page: Page) {
  await page.goto("");
  await expect(page.getByLabel("Message #general")).toBeVisible();
  await page.evaluate(() => {
    const saved: unknown[] = [];
    window.detailsSaved = saved;
    const answer = window.fake.handle.bind(window.fake);
    window.fake.handle = (command, args) => {
      if (command === "set_profile_details" && args && "details" in args) saved.push(args.details);
      return answer(command, args);
    };
  });
  await page.keyboard.press("Control+,");
  return page.getByRole("dialog", { name: "Settings" });
}

const savedDetails = (page: Page) => page.evaluate(() => window.detailsSaved ?? []);

test("the profile saves every field at once", async ({ page }) => {
  const settings = await openProfile(page);
  await settings.getByLabel("Pronouns").fill("she/her");
  await settings.getByLabel("Title").fill("Founder");
  await settings.getByLabel("Bio").fill("Writes the first programs.");
  await settings.getByLabel("Email").fill("ada@example.com");
  await settings.getByLabel("Website").fill("ada.example.com");
  await settings.getByLabel("Location").fill("London office");
  await settings.getByRole("button", { name: "Save profile" }).click();

  await expect(page.getByText("Your profile is updated for everyone.")).toBeVisible();
  expect(await savedDetails(page)).toEqual([
    {
      title: "Founder",
      pronouns: "she/her",
      bio: "Writes the first programs.",
      email: "ada@example.com",
      phone: "",
      website: "https://ada.example.com",
      location: "London office",
      time_zone: "",
      working_hours: null,
    },
  ]);
  await expect(settings.getByRole("button", { name: "Save profile" })).toBeDisabled();
});

test("a bad website or email is pointed out beside it and nothing is saved", async ({ page }) => {
  const settings = await openProfile(page);
  const website = settings.getByLabel("Website");
  await website.fill("javascript:alert(1)");
  await expect(settings.getByText(/Use a web address/)).toHaveCount(0);
  await website.blur();
  await expect(website).toHaveAccessibleDescription("Use a web address like example.com.");
  await expect(website).toHaveAttribute("aria-invalid", "true");

  await settings.getByLabel("Email").fill("ada@example");
  await settings.getByRole("button", { name: "Save profile" }).click();
  await expect(settings.getByLabel("Email")).toBeFocused();
  await expect(settings.getByLabel("Email")).toHaveAccessibleDescription(/name@example\.com/);
  expect(await savedDetails(page)).toEqual([]);

  await settings.getByLabel("Email").fill("ada@example.com");
  await website.fill("example.com/ada");
  await website.blur();
  await expect(website).toHaveValue("https://example.com/ada");
  await expect(website).toHaveAttribute("aria-invalid", "false");
  await settings.getByRole("button", { name: "Save profile" }).click();
  await expect.poll(() => savedDetails(page)).toHaveLength(1);
});

test("a website with a user name before its host is refused beside it", async ({ page }) => {
  const settings = await openProfile(page);
  const website = settings.getByLabel("Website");
  await website.fill("https://bank.example@evil.example/login");
  await website.blur();
  await expect(website).toHaveAttribute("aria-invalid", "true");
  await settings.getByLabel("Email").fill("ceo\u{E0041}@corp.example");
  await settings.getByLabel("Email").blur();
  await expect(settings.getByLabel("Email")).toHaveAttribute("aria-invalid", "true");
  await settings.getByRole("button", { name: "Save profile" }).click();
  expect(await savedDetails(page)).toEqual([]);
});

test("the time zone picker suggests this computer's zone and filters by city", async ({ page }) => {
  const settings = await openProfile(page);
  const zone = settings.getByLabel("Time zone");
  await expect(zone).toHaveText(/Not shared/);
  await zone.click();
  const options = page.getByRole("listbox", { name: "Time zones" }).getByRole("option");
  await expect(options.first()).toHaveAccessibleName("Use this computer's time zone (Europe/Lisbon)");

  await page.keyboard.press("Enter");
  await expect(zone).toHaveText(/Lisbon · GMT\+0/);
  await expect(zone).toBeFocused();

  await zone.click();
  await page.getByLabel("Search time zones").fill("tokyo");
  await expect(options).toHaveCount(1);
  await expect(options.first()).toHaveAccessibleName("Tokyo · GMT+9 (Asia/Tokyo)");
  await page.keyboard.press("Enter");
  await expect(zone).toHaveText(/Tokyo · GMT\+9/);

  await zone.click();
  await page.getByRole("option", { name: "Don't share" }).click();
  await expect(zone).toHaveText(/Not shared/);
});

test("the picker's list follows the arrow keys and finds zones by offset", async ({ page }) => {
  const settings = await openProfile(page);
  await settings.getByLabel("Time zone").click();
  const search = page.getByLabel("Search time zones");
  await search.fill("gmt+5:30");
  await expect(page.getByRole("option", { name: /Kolkata/ })).toBeVisible();
  await search.fill("");
  await page.keyboard.press("ArrowDown");
  const active = await search.getAttribute("aria-activedescendant");
  await expect(page.locator(`[id="${active}"]`)).toHaveAttribute("aria-selected", "true");
  await expect(page.locator(`[id="${active}"]`)).not.toHaveAccessibleName(/this computer/);
});

test("sharing working hours suggests weekdays nine to five, and remembers changes", async ({ page }) => {
  const settings = await openProfile(page);
  const share = settings.getByRole("switch", { name: "Share working hours" });
  await expect(share).not.toBeChecked();
  await expect(settings.getByRole("group", { name: "Working days" })).toHaveCount(0);

  await share.click();
  const days = settings.getByRole("group", { name: "Working days" });
  for (const day of ["Monday", "Tuesday", "Wednesday", "Thursday", "Friday"])
    await expect(days.getByRole("button", { name: day })).toHaveAttribute("aria-pressed", "true");
  for (const day of ["Saturday", "Sunday"])
    await expect(days.getByRole("button", { name: day })).toHaveAttribute("aria-pressed", "false");
  const start = settings.getByRole("combobox", { name: "Start" });
  const end = settings.getByRole("combobox", { name: "End" });
  await expect(start).toHaveText("9:00 AM");
  await expect(end).toHaveText("5:00 PM");

  await days.getByRole("button", { name: "Saturday" }).click();
  await end.click();
  await page.getByRole("option", { name: "2:00 AM (next day)" }).click();
  await share.click();
  await share.click();
  await expect(days.getByRole("button", { name: "Saturday" })).toHaveAttribute("aria-pressed", "true");
  await expect(end).toHaveText("2:00 AM (next day)");

  await settings.getByRole("button", { name: "Save profile" }).click();
  await expect
    .poll(() => savedDetails(page))
    .toEqual([expect.objectContaining({ working_hours: { days: 0b011_1111, start_minute: 540, end_minute: 120 } })]);
});

test("working hours need a day", async ({ page }) => {
  const settings = await openProfile(page);
  await settings.getByRole("switch", { name: "Share working hours" }).click();
  const days = settings.getByRole("group", { name: "Working days" });
  for (const day of ["Monday", "Tuesday", "Wednesday", "Thursday", "Friday"])
    await days.getByRole("button", { name: day }).click();
  await expect(settings.getByText("Pick at least one day.")).toBeVisible();
  await settings.getByRole("button", { name: "Save profile" }).click();
  expect(await savedDetails(page)).toEqual([]);
});

test("the completion line counts what's done and goes once it's all there", async ({ page }) => {
  const settings = await openProfile(page);
  const progress = settings.getByRole("progressbar", { name: "Profile completed" });
  await expect(progress).toHaveAttribute("aria-valuenow", "0");
  await expect(settings.getByText(/Add your role and time zone/)).toBeVisible();

  await settings.getByLabel("Title").fill("Founder");
  await settings.getByRole("button", { name: "Save profile" }).click();
  await expect(progress).toHaveAttribute("aria-valuenow", "1");

  await page.evaluate(() =>
    window.fake.setMemberDetails(window.fake.memberNamed("Ada"), {
      title: "Founder",
      bio: "Hi",
      time_zone: "Europe/London",
      working_hours: { days: 31, start_minute: 540, end_minute: 1020 },
    }),
  );
  await expect(progress).toHaveCount(0);
});

test("a dismissed completion line stays away on this device", async ({ page }) => {
  const settings = await openProfile(page);
  await settings.getByRole("button", { name: "Dismiss" }).click();
  await expect(settings.getByText(/Add your role and time zone/)).toHaveCount(0);

  await page.reload();
  await expect(page.getByLabel("Message #general")).toBeVisible();
  await page.keyboard.press("Control+,");
  await expect(page.getByRole("dialog", { name: "Settings" }).getByLabel("Display name")).toBeVisible();
  await expect(page.getByText(/Add your role and time zone/)).toHaveCount(0);
});

test("details the core would refuse fail in the fake too, so a UI slip can't pass a test", async ({ page }) => {
  await openProfile(page);
  const refusal = (details: Record<string, unknown>) =>
    page.evaluate(
      (details) =>
        window.__TAURI_INTERNALS__.invoke("set_profile_details", { details }).then(
          () => "saved",
          (error: unknown) => String(error),
        ),
      details,
    );
  const empty = {
    title: "",
    pronouns: "",
    bio: "",
    email: "",
    phone: "",
    website: "",
    location: "",
    time_zone: "",
    working_hours: null,
  };
  expect(await refusal({ ...empty, website: "javascript:alert(1)" })).toMatch(/website starts with https/);
  expect(await refusal({ ...empty, website: "https://bank.example@evil.example" })).toMatch(/website/);
  expect(await refusal({ ...empty, email: "ada@example" })).toMatch(/email address/);
  expect(await refusal({ ...empty, phone: "call me" })).toMatch(/phone number/);
  expect(await refusal({ ...empty, website: "https://example.com/a?b#c", email: "ada@example.com" })).toBe("saved");
});

test("Save takes the click even when a field's error pushes it down as you press it", async ({ page }) => {
  const settings = await openProfile(page);
  // A longer error than hint (another font or language) grows the form as the field loses focus.
  await page.addStyleTag({ content: '[id$="-error"] { letter-spacing: 0.25em }' });
  const email = settings.getByLabel("Email");
  await email.fill("ada@example");
  await email.evaluate((input) => {
    const panel = input.closest('[role="tabpanel"]');
    if (!(panel instanceof HTMLElement)) throw new Error("the email field is outside a settings panel");
    panel.scrollTop += input.getBoundingClientRect().bottom - panel.getBoundingClientRect().top - 4;
  });
  const save = await settings.getByRole("button", { name: "Save profile" }).boundingBox();
  if (!save) throw new Error("Save profile has no box");

  await page.mouse.click(save.x + save.width / 2, save.y + save.height / 2);

  await expect(email).toBeFocused();
  await expect(email).toHaveAccessibleDescription(/name@example\.com/);
  expect(await savedDetails(page)).toEqual([]);
});
