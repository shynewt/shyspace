import { expect, type Page, test } from "@playwright/test";

/** The app finds 0.2.0 downloaded at launch. */
async function openWithUpdate(page: Page) {
  await page.addInitScript(() => {
    window.prepareFake = (fake) => {
      fake.readyUpdate = "0.2.0";
    };
  });
  await page.goto("");
  await expect(page.getByLabel("Message #general")).toBeVisible();
}

function prompt(page: Page) {
  return page.getByRole("region", { name: "Update ready" });
}

test("a downloaded update waits in the sidebar until you restart", async ({ page }) => {
  await openWithUpdate(page);
  await expect(prompt(page)).toContainText("shyspace 0.2.0 is downloaded.");
  await page.evaluate(() => {
    window.fake.holdRestart = true;
  });
  await prompt(page).getByRole("button", { name: "Restart" }).click();
  // However long the system's password prompt stays open, it can't be pressed twice.
  await expect(prompt(page).getByRole("button", { name: "Restarting…" })).toBeDisabled();
  await page.evaluate(() => window.fake.finishRestart());
  await expect.poll(() => page.evaluate(() => window.fake.restartedInto)).toBe("0.2.0");
  await expect(prompt(page).getByRole("button", { name: "Restarting…" })).toBeDisabled();
});

test("an install that fails says why, and Restart can be tried again", async ({ page }) => {
  await openWithUpdate(page);
  await page.evaluate(() => {
    window.fake.holdRestart = true;
    window.fake.restartFails = "the administrator password was declined";
  });
  await prompt(page).getByRole("button", { name: "Restart" }).click();
  await expect(prompt(page).getByRole("button", { name: "Restarting…" })).toBeDisabled();
  await page.evaluate(() => window.fake.finishRestart());
  await expect(prompt(page).getByRole("alert")).toHaveText("the administrator password was declined");

  await page.evaluate(() => {
    window.fake.holdRestart = false;
    window.fake.restartFails = null;
  });
  await prompt(page).getByRole("button", { name: "Restart" }).click();
  await expect.poll(() => page.evaluate(() => window.fake.restartedInto)).toBe("0.2.0");
});

test("during a huddle, the restart waits until you leave it", async ({ page }) => {
  await openWithUpdate(page);
  await page.getByRole("button", { name: "Start a huddle" }).click();
  await expect(prompt(page)).toContainText("Restart after your huddle.");
  await expect(prompt(page).getByRole("button", { name: "Restart" })).toBeDisabled();

  await page.getByRole("region", { name: "Your huddle" }).getByRole("button", { name: "Leave" }).click();
  await expect(prompt(page).getByRole("button", { name: "Restart" })).toBeEnabled();
  await expect(prompt(page)).not.toContainText("after your huddle");
});

test("an update another restart already took says so", async ({ page }) => {
  await openWithUpdate(page);
  await page.evaluate(() => {
    window.fake.readyUpdate = null;
  });
  await prompt(page).getByRole("button", { name: "Restart" }).click();
  await expect(prompt(page)).toContainText("Already updating…");
  await expect(prompt(page).getByRole("button")).toHaveCount(0);
});

test("with the sidebar hidden, the header offers the restart", async ({ page }) => {
  await openWithUpdate(page);
  await page.keyboard.press("Control+\\");
  const pill = page.getByRole("group", { name: "Update ready" });
  await expect(pill).toContainText("Update ready");
  await pill.getByRole("button", { name: "Restart to update to shyspace 0.2.0" }).click();
  await expect.poll(() => page.evaluate(() => window.fake.restartedInto)).toBe("0.2.0");
});

test("with no update, nothing shows", async ({ page }) => {
  await page.goto("");
  await expect(page.getByLabel("Message #general")).toBeVisible();
  await expect(page.getByText("Update ready")).toHaveCount(0);
});

test("the card and the header pill share one restart", async ({ page }) => {
  await page.setViewportSize({ width: 720, height: 800 });
  await openWithUpdate(page);
  await page.evaluate(() => {
    window.fake.holdRestart = true;
    window.fake.restartFails = "the administrator password was declined";
  });
  const pill = page.getByRole("group", { name: "Update ready" });
  await pill.getByRole("button", { name: "Restart to update to shyspace 0.2.0" }).click();
  await page.getByRole("button", { name: "Show sidebar" }).click();
  await expect(prompt(page).getByRole("button", { name: "Restarting…" })).toBeDisabled();

  await page.evaluate(() => window.fake.finishRestart());
  await expect(prompt(page).getByRole("alert")).toHaveText("the administrator password was declined");
  await expect(prompt(page).getByRole("button", { name: "Restart" })).toBeEnabled();
  await page.keyboard.press("Escape");
  await expect(pill).toContainText("Couldn't update");
  await expect(pill).toContainText("the administrator password was declined");
});

test("a restart refused for a huddle the window doesn't show says so", async ({ page }) => {
  await openWithUpdate(page);
  // A huddle in another workspace on this device: the core refuses, though this one shows no call.
  await page.evaluate(() => {
    window.fake.huddleElsewhere = true;
  });
  await prompt(page).getByRole("button", { name: "Restart" }).click();
  await expect(prompt(page)).toContainText("Restart after your huddle.");
  await page.evaluate(() => {
    window.fake.huddleElsewhere = false;
  });
  await prompt(page).getByRole("button", { name: "Restart" }).click();
  await expect.poll(() => page.evaluate(() => window.fake.restartedInto)).toBe("0.2.0");
});
