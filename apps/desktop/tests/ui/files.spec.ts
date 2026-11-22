import { expect, test } from "@playwright/test";

const PNG = [
  137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 1, 0, 0, 0, 1, 8, 6, 0, 0, 0, 31, 21, 196, 137,
  0, 0, 0, 13, 73, 68, 65, 84, 120, 156, 99, 248, 207, 192, 240, 31, 0, 5, 0, 1, 255, 137, 153, 61, 29, 0, 0, 0, 0, 73,
  69, 78, 68, 174, 66, 96, 130,
];

test.beforeEach(async ({ page }) => {
  await page.goto("");
  await page.getByRole("button", { name: "general", exact: true }).click();
});

test("a picked file waits in the message box, then sends with the text", async ({ page }) => {
  await page.evaluate(() => {
    window.fake.files.pickNext = [{ name: "notes.txt", type: "text/plain", bytes: [104, 105] }];
  });
  await page.getByRole("button", { name: "Attach files" }).click();
  const waiting = page.getByRole("list", { name: "Files to send" });
  await expect(waiting.getByText("notes.txt")).toBeVisible();

  const composer = page.getByLabel("Message #general");
  await composer.fill("Here are my notes");
  await composer.press("Enter");

  const message = page.getByRole("article").filter({ hasText: "Here are my notes" });
  await expect(message.getByText("notes.txt")).toBeVisible();
  await expect(message.getByText("2 B")).toBeVisible();
  await expect(waiting).toHaveCount(0);
  await expect(composer).toHaveValue("");
});

test("files send without a caption, and a removed one stays behind", async ({ page }) => {
  await page.evaluate(() => {
    window.fake.files.pickNext = [
      { name: "keep.txt", type: "text/plain", bytes: [1] },
      { name: "drop.txt", type: "text/plain", bytes: [2] },
    ];
  });
  await page.getByRole("button", { name: "Attach files" }).click();
  await page.getByRole("button", { name: "Remove drop.txt" }).click();
  await page.getByRole("button", { name: "Send" }).click();

  await expect(page.getByRole("article").getByText("keep.txt")).toBeVisible();
  await expect(page.getByText("drop.txt")).toHaveCount(0);
});

test("an image shows in the message, and only real images do", async ({ page }) => {
  await page.evaluate((png) => {
    window.fake.files.pickNext = [
      { name: "photo.png", type: "image/png", bytes: png },
      { name: "fake.png", type: "image/png", bytes: [60, 115, 118, 103, 62] },
    ];
  }, PNG);
  await page.getByRole("button", { name: "Attach files" }).click();
  await page.getByRole("button", { name: "Send" }).click();

  const message = page.getByRole("article").last();
  await expect(message.getByRole("img", { name: "photo.png" })).toBeVisible();
  // Its name says PNG, its bytes don't: a card, never an image.
  await expect(message.getByRole("img", { name: "fake.png" })).toHaveCount(0);
  await expect(message.getByText("fake.png")).toBeVisible();
});

test("dropped files land in the message box", async ({ page }) => {
  await page.getByLabel("Message #general").focus();
  await page.evaluate(() => window.fake.files.dragOver(true));
  await expect(page.getByText(/Drop to attach/)).toBeVisible();
  await page.evaluate(() => window.fake.files.drop([{ name: "plan.pdf", type: "application/pdf", bytes: [37, 80] }]));
  await expect(page.getByText(/Drop to attach/)).toHaveCount(0);
  await expect(page.getByRole("list", { name: "Files to send" }).getByText("plan.pdf")).toBeVisible();
});

test("a pasted image is attached instead of pasted as text", async ({ page }) => {
  const composer = page.getByLabel("Message #general");
  await composer.focus();
  await page.evaluate((png) => {
    const transfer = new DataTransfer();
    transfer.items.add(new File([new Uint8Array(png)], "screenshot.png", { type: "image/png" }));
    const input = document.activeElement;
    input?.dispatchEvent(new ClipboardEvent("paste", { clipboardData: transfer, bubbles: true, cancelable: true }));
  }, PNG);
  await expect(page.getByRole("list", { name: "Files to send" }).getByText("screenshot.png")).toBeVisible();
  await expect(composer).toHaveValue("");
});

test("a file that fails to send stays in the box with its text", async ({ page }) => {
  await page.evaluate(() => {
    window.fake.files.pickNext = [{ name: "report.pdf", type: "application/pdf", bytes: [1, 2, 3] }];
    window.fake.files.failNextSend = true;
  });
  await page.getByRole("button", { name: "Attach files" }).click();
  const composer = page.getByLabel("Message #general");
  await composer.fill("The report");
  await composer.press("Enter");

  await expect(page.getByText("couldn't reach the bucket")).toBeVisible();
  await expect(page.getByRole("list", { name: "Files to send" }).getByText("report.pdf")).toBeVisible();
  await expect(composer).toHaveValue("The report");
  await composer.press("Enter");
  await expect(page.getByRole("article").filter({ hasText: "The report" }).getByText("report.pdf")).toBeVisible();
});

test("too many files can't be sent, and the box says why", async ({ page }) => {
  await page.evaluate(() => {
    window.fake.files.pickNext = Array.from({ length: 11 }, (_, index) => ({
      name: `file-${index}.txt`,
      type: "text/plain",
      bytes: [index],
    }));
  });
  await page.getByRole("button", { name: "Attach files" }).click();
  await expect(page.getByRole("alert").filter({ hasText: "at most 10 files" })).toBeVisible();
  await expect(page.getByRole("button", { name: "Send" })).toBeDisabled();
});

test("someone else's file can be saved but not deleted; your own can be deleted", async ({ page }) => {
  // As Grace, a regular member: Ada's file isn't hers to delete.
  await page.goto("?start=member");
  await page.getByRole("button", { name: "general", exact: true }).click();
  await page.evaluate(() => {
    const ada = window.fake.memberNamed("Ada");
    const message = window.fake.receiveMessage(ada, "general", "Minutes attached");
    window.fake.files.receive(ada, message, { name: "minutes.md", type: "text/markdown", bytes: [35] });
  });
  const theirs = page.getByRole("article").filter({ hasText: "Minutes attached" });
  await theirs.getByRole("button", { name: "Save minutes.md" }).click();
  expect(await page.evaluate(() => window.fake.files.saved.length)).toBe(1);
  await expect(theirs.getByRole("button", { name: "Delete minutes.md" })).toHaveCount(0);

  await page.evaluate(() => {
    window.fake.files.pickNext = [{ name: "mine.txt", type: "text/plain", bytes: [1] }];
  });
  await page.getByRole("button", { name: "Attach files" }).click();
  await page.getByLabel("Message #general").fill("Mine");
  await page.getByLabel("Message #general").press("Enter");
  const mine = page.getByRole("article").filter({ hasText: "Mine" });
  await mine.getByRole("button", { name: "Delete mine.txt" }).click();
  await page.getByRole("button", { name: "Delete file" }).click();
  await expect(mine.getByText("mine.txt")).toHaveCount(0);
  await expect(mine.getByText("Mine")).toBeVisible();
});

test("a member's picture replaces their initials", async ({ page }) => {
  await page.evaluate((png) => {
    const grace = window.fake.addMember("Grace");
    window.fake.files.setPicture(grace, png);
    window.fake.receiveMessage(grace, "general", "Hello with a face");
  }, PNG);
  const message = page.getByRole("article").filter({ hasText: "Hello with a face" });
  await expect(message.locator("img[src^='blob:']")).toHaveCount(1);
});

test("the files view lists what was shared, by channel, and filters it", async ({ page }) => {
  await page.evaluate((png) => {
    const grace = window.fake.addMember("Grace");
    const first = window.fake.receiveMessage(grace, "general", "Logo");
    window.fake.files.receive(grace, first, { name: "logo.png", type: "image/png", bytes: png });
    const second = window.fake.receiveMessage(grace, "general", "Budget");
    window.fake.files.receive(grace, second, { name: "budget.pdf", type: "application/pdf", bytes: [37, 80] });
  }, PNG);
  await page.getByRole("button", { name: "Files", exact: true }).click();
  await expect(page.getByRole("heading", { name: "Files" })).toBeVisible();
  await expect(page.getByText("logo.png")).toBeVisible();
  await expect(page.getByText("budget.pdf")).toBeVisible();

  await page.getByRole("radio", { name: "Images" }).check();
  await expect(page.getByText("budget.pdf")).toHaveCount(0);
  await expect(page.getByText("logo.png")).toBeVisible();

  await page.getByRole("radio", { name: "All" }).check();
  await page.getByLabel("Search files by name").fill("budg");
  await expect(page.getByText("logo.png")).toHaveCount(0);
  await page.getByRole("button", { name: "Show budget.pdf in chat" }).click();
  await expect(page.getByLabel("Message #general")).toBeVisible();
  await expect(page.getByRole("article").filter({ hasText: "Budget" })).toBeVisible();
});

test("a chosen picture is cropped square, re-encoded and shared; it can be removed", async ({ page }) => {
  await page.evaluate(() => {
    const answer = window.fake.handle.bind(window.fake);
    window.fake.handle = (command, args) => {
      if (command === "set_profile_picture" && args && "image" in args) {
        (window as unknown as { sharedPicture: number[] }).sharedPicture = args.image as number[];
      }
      return answer(command, args);
    };
  });
  await page.keyboard.press("Control+,");
  const settings = page.getByRole("dialog", { name: "Settings" });
  await settings.getByLabel("Choose a picture").setInputFiles({
    name: "holiday.png",
    mimeType: "image/png",
    buffer: Buffer.from(PNG),
  });
  await expect(page.getByText("Your picture is updated for everyone.")).toBeVisible();
  const shared = await page.evaluate(() => (window as unknown as { sharedPicture: number[] }).sharedPicture);
  // A fresh JPEG: nothing of the original file's own data travels.
  expect(shared.slice(0, 3)).toEqual([0xff, 0xd8, 0xff]);

  await settings.getByRole("button", { name: "Remove" }).click();
  await expect(page.getByText("Your picture is removed.")).toBeVisible();
  await expect(settings.getByRole("button", { name: "Add a picture" })).toBeVisible();
});

test("an image claiming enormous dimensions shows as a card, never decoded", async ({ page }) => {
  // A real PNG signature and header, claiming 60000 × 60000 pixels.
  const huge = [...PNG];
  huge.splice(16, 8, 0, 0, 0xea, 0x60, 0, 0, 0xea, 0x60);
  await page.evaluate((bytes) => {
    window.fake.files.pickNext = [{ name: "bomb.png", type: "image/png", bytes }];
  }, huge);
  await page.getByRole("button", { name: "Attach files" }).click();
  await page.getByRole("button", { name: "Send" }).click();
  const message = page.getByRole("article").last();
  await expect(message.getByText("bomb.png")).toBeVisible();
  await expect(message.getByRole("img", { name: "bomb.png" })).toHaveCount(0);
});

test("the files view previews small images", async ({ page }) => {
  await page.evaluate((png) => {
    const grace = window.fake.addMember("Grace");
    const message = window.fake.receiveMessage(grace, "general", "Logo");
    window.fake.files.receive(grace, message, { name: "logo.png", type: "image/png", bytes: png });
  }, PNG);
  await page.getByRole("button", { name: "Files", exact: true }).click();
  await expect(page.getByRole("main").locator("img[src^='blob:']")).toHaveCount(1);
});
