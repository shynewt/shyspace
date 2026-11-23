import { expect, test } from "@playwright/test";
import { allowClipboard } from "./support";

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
  await theirs.getByRole("button", { name: "More actions for minutes.md" }).click();
  await expect(page.getByRole("menuitem", { name: "Save…" })).toBeVisible();
  await expect(page.getByRole("menuitem", { name: "Delete…" })).toHaveCount(0);
  await page.keyboard.press("Escape");

  await page.evaluate(() => {
    window.fake.files.pickNext = [{ name: "mine.txt", type: "text/plain", bytes: [1] }];
  });
  await page.getByRole("button", { name: "Attach files" }).click();
  await page.getByLabel("Message #general").fill("Mine");
  await page.getByLabel("Message #general").press("Enter");
  const mine = page.getByRole("article").filter({ hasText: "Mine" });
  await mine.getByRole("button", { name: "More actions for mine.txt" }).click();
  await page.getByRole("menuitem", { name: "Delete…" }).click();
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

test("the files view lists what was shared and filters it", async ({ page }) => {
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

test("clicking an image opens it large, and Esc closes it", async ({ page }) => {
  await page.evaluate((png) => {
    window.fake.files.pickNext = [{ name: "photo.png", type: "image/png", bytes: png }];
  }, PNG);
  await page.getByRole("button", { name: "Attach files" }).click();
  await page.getByRole("button", { name: "Send" }).click();
  await page.getByRole("button", { name: "View photo.png" }).click();
  const viewer = page.getByRole("dialog", { name: "photo.png" });
  await expect(viewer.getByRole("img", { name: "photo.png" })).toBeVisible();
  await expect(viewer.getByRole("button", { name: "Save photo.png" })).toBeVisible();
  // The viewer takes focus once it's open, which is what lets the first Esc close it.
  await expect(viewer).toBeFocused();
  await page.keyboard.press("Escape");
  await expect(viewer).toHaveCount(0);
});

test("a waiting image shows its own thumbnail in the message box", async ({ page }) => {
  await page.evaluate((png) => {
    window.fake.files.pickNext = [
      { name: "photo.png", type: "image/png", bytes: png },
      { name: "notes.txt", type: "text/plain", bytes: [1] },
    ];
  }, PNG);
  await page.getByRole("button", { name: "Attach files" }).click();
  const waiting = page.getByRole("list", { name: "Files to send" });
  await expect(waiting.locator("img[src^='blob:']")).toHaveCount(1);
  await expect(waiting.getByText("notes.txt")).toBeVisible();
});

test("in the files view, a whole image row opens the viewer", async ({ page }) => {
  await page.evaluate((png) => {
    const grace = window.fake.addMember("Grace");
    const message = window.fake.receiveMessage(grace, "general", "Logo");
    window.fake.files.receive(grace, message, { name: "logo.png", type: "image/png", bytes: png });
  }, PNG);
  await page.getByRole("button", { name: "Files", exact: true }).click();
  await page.getByText("logo.png").click();
  await expect(page.getByRole("dialog", { name: "logo.png" }).getByRole("img", { name: "logo.png" })).toBeVisible();
});

test("files dropped after the thread closes go to the channel's message box", async ({ page }) => {
  await page.setViewportSize({ width: 1600, height: 900 });
  await page.evaluate(() => {
    const grace = window.fake.addMember("Grace");
    const root = window.fake.receiveMessage(grace, "general", "Plans for the offsite");
    window.fake.receiveReply(grace, root, "Lisbon?");
  });
  await page
    .getByRole("log")
    .getByRole("button", { name: /1 reply/ })
    .click();
  const thread = page.getByRole("complementary", { name: "Thread" });
  await thread.getByLabel("Reply in thread").focus();
  await thread.getByRole("button", { name: "Close" }).click();
  await expect(thread).toHaveCount(0);

  await page.evaluate(() => window.fake.files.drop([{ name: "agenda.md", type: "text/markdown", bytes: [35] }]));
  await expect(page.getByRole("list", { name: "Files to send" }).getByText("agenda.md")).toBeVisible();
});

test("a text file opens as plain text, and markdown opens formatted", async ({ page }) => {
  await page.evaluate(() => {
    const grace = window.fake.addMember("Grace");
    const first = window.fake.receiveMessage(grace, "general", "Logs");
    const encode = (text: string) => Array.from(new TextEncoder().encode(text));
    window.fake.files.receive(grace, first, {
      name: "server.log",
      type: "text/plain",
      bytes: encode("<b>not bold</b>\nline two"),
    });
    const second = window.fake.receiveMessage(grace, "general", "Notes");
    window.fake.files.receive(grace, second, {
      name: "notes.md",
      type: "text/markdown",
      bytes: encode("# Plan\n\n- **Ship** files"),
    });
  });
  await page.getByRole("button", { name: "View server.log" }).click();
  const log = page.getByRole("dialog", { name: "server.log" });
  await expect(log.getByText("<b>not bold</b>")).toBeVisible();
  await expect(log.locator("b")).toHaveCount(0);
  await page.keyboard.press("Escape");
  await expect(log).toHaveCount(0);

  await page.getByRole("button", { name: "View notes.md" }).click();
  const notes = page.getByRole("dialog", { name: "notes.md" });
  const heading = notes.getByRole("heading", { name: "Plan" });
  await expect(heading).toBeVisible();
  // Reads like a page: a heading stands out from the text below it.
  const sizes = await notes.evaluate((dialog) => {
    const size = (selector: string) => {
      const element = dialog.querySelector(selector);
      if (!element) throw new Error(`no ${selector} in the preview`);
      return Number.parseFloat(getComputedStyle(element).fontSize);
    };
    return { heading: size("h1"), text: size("li") };
  });
  expect(sizes.heading).toBeGreaterThan(sizes.text * 1.5);
  await expect(notes.locator("strong", { hasText: "Ship" })).toBeVisible();
});

test("a file named like text but holding binary says so instead of showing garbage", async ({ page }) => {
  await page.evaluate(() => {
    const grace = window.fake.addMember("Grace");
    const message = window.fake.receiveMessage(grace, "general", "Data");
    window.fake.files.receive(grace, message, { name: "data.txt", type: "text/plain", bytes: [0, 159, 146, 150] });
  });
  await page.getByRole("button", { name: "View data.txt" }).click();
  await expect(page.getByText("This isn't readable as text.")).toBeVisible();
});

test("right-clicking an image copies it, without opening the message's menu", async ({
  page,
  context,
  browserName,
}) => {
  await allowClipboard(context, browserName);
  await page.evaluate((png) => {
    window.fake.files.pickNext = [{ name: "photo.png", type: "image/png", bytes: png }];
  }, PNG);
  await page.getByRole("button", { name: "Attach files" }).click();
  await page.getByRole("button", { name: "Send" }).click();
  await page.getByRole("button", { name: "View photo.png" }).click({ button: "right" });
  await expect(page.getByRole("menuitem", { name: "Copy image" })).toBeVisible();
  await expect(page.getByRole("menuitem", { name: "Save for later" })).toHaveCount(0);
  await page.getByRole("menuitem", { name: "Copy image" }).click();
  await expect(page.getByText("Image copied.")).toBeVisible();
  // Playwright's WebKit keeps text on its clipboard but no images (its read
  // comes back empty), so what lands there is checked in Chromium here and
  // in the real app by hand.
  if (browserName === "chromium") {
    const types = await page.evaluate(async () => (await navigator.clipboard.read()).flatMap((item) => item.types));
    expect(types).toContain("image/png");
  }
});

test("file cards say what kind of file they are", async ({ page }) => {
  await page.evaluate(() => {
    window.fake.files.pickNext = [
      { name: "report.pdf", type: "application/pdf", bytes: [37, 80] },
      { name: "notes.md", type: "text/markdown", bytes: [35] },
    ];
  });
  await page.getByRole("button", { name: "Attach files" }).click();
  await page.getByRole("button", { name: "Send" }).click();
  const message = page.getByRole("article").last();
  await expect(message.getByText("PDF", { exact: true })).toBeVisible();
  await expect(message.getByText("MD", { exact: true })).toBeVisible();
});

test("you can rename a file you shared; the name before the extension is selected", async ({ page }) => {
  await page.evaluate(() => {
    window.fake.files.pickNext = [{ name: "IMG_4821.png", type: "image/png", bytes: [1, 2] }];
  });
  await page.getByRole("button", { name: "Attach files" }).click();
  await page.getByRole("button", { name: "Send" }).click();
  await page.getByRole("button", { name: "More actions for IMG_4821.png" }).click();
  await page.getByRole("menuitem", { name: "Rename…" }).click();
  const dialog = page.getByRole("dialog", { name: "Rename file" });
  const field = dialog.getByLabel("Name");
  await expect(field).toBeFocused();
  // Typing replaces only the selected part, keeping ".png".
  await page.keyboard.type("Whiteboard");
  await expect(field).toHaveValue("Whiteboard.png");
  await dialog.getByRole("button", { name: "Rename" }).click();
  await expect(dialog).toHaveCount(0);
  await expect(page.getByRole("article").last().getByText("Whiteboard.png")).toBeVisible();
});

test("someone else's file has no rename", async ({ page }) => {
  await page.evaluate(() => {
    const grace = window.fake.addMember("Grace");
    const message = window.fake.receiveMessage(grace, "general", "Minutes");
    window.fake.files.receive(grace, message, { name: "minutes.md", type: "text/markdown", bytes: [35] });
  });
  await page.getByRole("button", { name: "More actions for minutes.md" }).click();
  await expect(page.getByRole("menuitem", { name: "Save…" })).toBeVisible();
  await expect(page.getByRole("menuitem", { name: "Rename…" })).toHaveCount(0);
});

/** A small, valid PDF with `pages` pages, each saying its number. */
function pdfWithPages(pages: number, mediaBox = "0 0 612 792"): number[] {
  const objects: string[] = [];
  const pageIds = Array.from({ length: pages }, (_, index) => 4 + index * 2);
  objects[1] = "<< /Type /Catalog /Pages 2 0 R >>";
  objects[2] = `<< /Type /Pages /Kids [${pageIds.map((id) => `${id} 0 R`).join(" ")}] /Count ${pages} >>`;
  objects[3] = "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>";
  for (const [index, id] of pageIds.entries()) {
    const content = `BT /F1 48 Tf 72 700 Td (Page ${index + 1}) Tj ET`;
    objects[id] =
      `<< /Type /Page /Parent 2 0 R /MediaBox [${mediaBox}] /Resources << /Font << /F1 3 0 R >> >> /Contents ${id + 1} 0 R >>`;
    objects[id + 1] = `<< /Length ${content.length} >>\nstream\n${content}\nendstream`;
  }
  let pdf = "%PDF-1.4\n";
  const offsets: number[] = [];
  for (let id = 1; id < objects.length; id++) {
    offsets[id] = pdf.length;
    pdf += `${id} 0 obj\n${objects[id]}\nendobj\n`;
  }
  const xref = pdf.length;
  pdf += `xref\n0 ${objects.length}\n0000000000 65535 f \n`;
  for (let id = 1; id < objects.length; id++) pdf += `${String(offsets[id]).padStart(10, "0")} 00000 n \n`;
  pdf += `trailer\n<< /Size ${objects.length} /Root 1 0 R >>\nstartxref\n${xref}\n%%EOF\n`;
  return Array.from(new TextEncoder().encode(pdf));
}

test("a PDF opens drawn page by page", async ({ page }) => {
  await page.evaluate((bytes) => {
    const grace = window.fake.addMember("Grace");
    const message = window.fake.receiveMessage(grace, "general", "The contract");
    window.fake.files.receive(grace, message, { name: "contract.pdf", type: "application/pdf", bytes });
  }, pdfWithPages(2));
  await page.getByRole("button", { name: "View contract.pdf" }).click();
  const viewer = page.getByRole("dialog", { name: "contract.pdf" });
  await expect(viewer.getByRole("img", { name: "Page 1 of contract.pdf" })).toBeVisible();
  await expect(viewer.getByRole("img", { name: "Page 2 of contract.pdf" })).toBeAttached();
  const box = await viewer.getByRole("img", { name: "Page 1 of contract.pdf" }).boundingBox();
  expect(box?.height).toBeGreaterThan(300);
  await expect(viewer.getByText("couldn't be shown")).toHaveCount(0);
});

test("a long PDF shows its first 30 pages and says so", async ({ page }) => {
  await page.evaluate((bytes) => {
    const grace = window.fake.addMember("Grace");
    const message = window.fake.receiveMessage(grace, "general", "The manual");
    window.fake.files.receive(grace, message, { name: "manual.pdf", type: "application/pdf", bytes });
  }, pdfWithPages(31));
  await page.getByRole("button", { name: "View manual.pdf" }).click();
  const viewer = page.getByRole("dialog", { name: "manual.pdf" });
  await expect(viewer.getByText("Showing the first 30 of 31 pages.")).toBeVisible({ timeout: 20_000 });
  await expect(viewer.getByRole("img", { name: /^Page \d+ of manual\.pdf$/ })).toHaveCount(30);
});

test("a file named like a PDF that isn't one says so", async ({ page }) => {
  await page.evaluate(() => {
    const grace = window.fake.addMember("Grace");
    const message = window.fake.receiveMessage(grace, "general", "Fake");
    window.fake.files.receive(grace, message, { name: "fake.pdf", type: "application/pdf", bytes: [60, 104, 116] });
  });
  await page.getByRole("button", { name: "View fake.pdf" }).click();
  await expect(page.getByText("This isn't a PDF.")).toBeVisible();
});

test("a page too large to draw says so in its place", async ({ page }) => {
  await page.evaluate(
    (bytes) => {
      const grace = window.fake.addMember("Grace");
      const message = window.fake.receiveMessage(grace, "general", "A banner");
      window.fake.files.receive(grace, message, { name: "banner.pdf", type: "application/pdf", bytes });
    },
    pdfWithPages(1, "0 0 612 100000"),
  );
  await page.getByRole("button", { name: "View banner.pdf" }).click();
  await expect(page.getByText("Page 1 is too large to preview.")).toBeVisible();
});

test("the keyboard scrolls a long PDF", async ({ page }) => {
  await page.evaluate((bytes) => {
    const grace = window.fake.addMember("Grace");
    const message = window.fake.receiveMessage(grace, "general", "The manual");
    window.fake.files.receive(grace, message, { name: "manual.pdf", type: "application/pdf", bytes });
  }, pdfWithPages(4));
  await page.getByRole("button", { name: "View manual.pdf" }).click();
  const viewer = page.getByRole("dialog", { name: "manual.pdf" });
  await expect(viewer.getByRole("img", { name: "Page 4 of manual.pdf" })).toBeAttached();
  await page.keyboard.press("PageDown");
  await expect.poll(() => viewer.evaluate((dialog) => dialog.scrollTop)).toBeGreaterThan(0);
});

test("reopening rename starts from the file's current name", async ({ page }) => {
  await page.evaluate(() => {
    window.fake.files.pickNext = [{ name: "notes.txt", type: "text/plain", bytes: [1] }];
  });
  await page.getByRole("button", { name: "Attach files" }).click();
  await page.getByRole("button", { name: "Send" }).click();
  const rename = async () => {
    await page.getByRole("button", { name: "More actions for notes.txt" }).click();
    await page.getByRole("menuitem", { name: "Rename…" }).click();
  };
  await rename();
  const dialog = page.getByRole("dialog", { name: "Rename file" });
  await page.keyboard.type("half typed");
  await dialog.getByRole("button", { name: "Cancel" }).click();
  await rename();
  await expect(dialog.getByLabel("Name")).toHaveValue("notes.txt");
});

test("the files view lists files newest first, by day, saying who shared each and where", async ({ page }) => {
  await page.clock.install({ time: new Date("2026-11-20T10:00:00") });
  await page.goto("");
  await page.evaluate(() => {
    const grace = window.fake.addMember("Grace");
    const old = window.fake.receiveMessage(grace, "general", "Old");
    window.fake.files.receive(grace, old, { name: "old.txt", type: "text/plain", bytes: [1] });
  });
  await page.clock.setSystemTime(new Date("2026-11-22T09:00:00"));
  await page.evaluate(() => {
    const grace = window.fake.memberNamed("Grace");
    const recent = window.fake.receiveMessage(grace, "general", "New");
    window.fake.files.receive(grace, recent, { name: "new.txt", type: "text/plain", bytes: [1] });
  });
  await page.getByRole("button", { name: "Files", exact: true }).click();

  const today = page.getByRole("region", { name: "Today" });
  const earlier = page.getByRole("region", { name: "Friday, November 20" });
  await expect(today.getByText("new.txt")).toBeVisible();
  await expect(earlier.getByText("old.txt")).toBeVisible();
  const [todayBox, earlierBox] = [await today.boundingBox(), await earlier.boundingBox()];
  expect(todayBox?.y ?? 0).toBeLessThan(earlierBox?.y ?? 0);
  await expect(today.getByRole("listitem")).toContainText("Grace");
  await expect(today.getByRole("listitem")).toContainText("general");
});

test("the files view filters by who shared, and Clear filters brings everything back", async ({ page }) => {
  await page.evaluate(() => {
    const grace = window.fake.addMember("Grace");
    const linus = window.fake.addMember("Linus");
    const first = window.fake.receiveMessage(grace, "general", "Plan");
    window.fake.files.receive(grace, first, { name: "plan.txt", type: "text/plain", bytes: [1] });
    const second = window.fake.receiveMessage(linus, "general", "Patch");
    window.fake.files.receive(linus, second, { name: "patch.diff", type: "text/plain", bytes: [1] });
  });
  await page.getByRole("button", { name: "Files", exact: true }).click();
  const sharedBy = page.getByRole("combobox", { name: "Shared by" });
  await expect(sharedBy).toHaveText("Shared by anyone");

  // From the keyboard: open the list, type to jump to a name, choose it.
  await sharedBy.focus();
  await page.keyboard.press("Enter");
  await expect(page.getByRole("option", { name: "Shared by anyone" })).toBeVisible();
  await page.keyboard.type("Lin");
  await expect(page.getByRole("option", { name: /Linus/ })).toBeFocused();
  await page.keyboard.press("Enter");
  await expect(sharedBy).toHaveText(/Linus/);
  await expect(page.getByText("patch.diff")).toBeVisible();
  await expect(page.getByText("plan.txt")).toHaveCount(0);

  await page.getByLabel("Search files by name").fill("nothing like this");
  await expect(page.getByText("No files match.")).toBeVisible();
  await page.getByRole("button", { name: "Clear filters" }).click();
  await expect(sharedBy).toHaveText("Shared by anyone");
  await expect(page.getByLabel("Search files by name")).toHaveValue("");
  await expect(page.getByText("plan.txt")).toBeVisible();
  await expect(page.getByText("patch.diff")).toBeVisible();
});

test("a file the app can't show opens a page that offers to save it", async ({ page }) => {
  await page.evaluate(() => {
    const grace = window.fake.addMember("Grace");
    const message = window.fake.receiveMessage(grace, "general", "Backup");
    window.fake.files.receive(grace, message, { name: "backup.zip", type: "application/zip", bytes: [80, 75] });
  });
  await page.getByRole("button", { name: "View backup.zip" }).click();
  const viewer = page.getByRole("dialog", { name: "backup.zip" });
  await expect(viewer.getByText("There's no preview for this kind of file.")).toBeVisible();
  await viewer.getByRole("button", { name: "Save", exact: true }).click();
  expect(await page.evaluate(() => window.fake.files.saved.length)).toBe(1);
});

test("the viewer says who shared the file and where, closes with its button, and leads to the message", async ({
  page,
}) => {
  await page.evaluate(() => {
    const grace = window.fake.addMember("Grace");
    const message = window.fake.receiveMessage(grace, "general", "Notes for today");
    window.fake.files.receive(grace, message, { name: "notes.txt", type: "text/plain", bytes: [104, 105] });
  });
  await page.getByRole("button", { name: "Files", exact: true }).click();
  await page.getByText("notes.txt").click();
  const viewer = page.getByRole("dialog", { name: "notes.txt" });
  await expect(viewer).toContainText("Grace");
  await expect(viewer).toContainText("general");
  // Opening focuses the viewer itself, not its first button, so the first Esc closes it.
  await expect(viewer).toBeFocused();

  const close = viewer.getByRole("button", { name: "Close", exact: true });
  await expect(close).toBeInViewport();
  await close.click();
  await expect(viewer).toHaveCount(0);

  await page.getByText("notes.txt").click();
  await viewer.getByRole("button", { name: "Show notes.txt in chat" }).click();
  await expect(viewer).toHaveCount(0);
  await expect(page.getByRole("article").filter({ hasText: "Notes for today" })).toBeVisible();
});

test("right-clicking a file opens its menu, in the files view and in chat", async ({ page }) => {
  await page.evaluate(() => {
    const grace = window.fake.addMember("Grace");
    const message = window.fake.receiveMessage(grace, "general", "The minutes");
    window.fake.files.receive(grace, message, { name: "minutes.md", type: "text/markdown", bytes: [35] });
  });
  await page.getByRole("button", { name: "View minutes.md" }).click({ button: "right" });
  await expect(page.getByRole("menuitem", { name: "Save…" })).toBeVisible();
  // The file's menu, not the message's.
  await expect(page.getByRole("menuitem", { name: "Copy link" })).toHaveCount(0);
  await page.keyboard.press("Escape");

  await page.getByRole("button", { name: "Files", exact: true }).click();
  await page.getByText("minutes.md").click({ button: "right" });
  await expect(page.getByRole("menuitem", { name: "Show in chat" })).toBeVisible();
  await expect(page.getByRole("menuitem", { name: "Save…" })).toBeVisible();
});

test("after a dialog opened from a file's menu closes, focus is back on the menu button", async ({ page }) => {
  await page.evaluate(() => {
    window.fake.files.pickNext = [{ name: "notes.txt", type: "text/plain", bytes: [1] }];
  });
  await page.getByRole("button", { name: "Attach files" }).click();
  await page.getByRole("button", { name: "Send" }).click();
  const more = page.getByRole("button", { name: "More actions for notes.txt" });
  await more.click();
  await page.getByRole("menuitem", { name: "Rename…" }).click();
  await page.getByRole("dialog", { name: "Rename file" }).getByRole("button", { name: "Cancel" }).click();
  await expect(more).toBeFocused();
});

test("a file row centers its thumbnail on the two lines of text", async ({ page }) => {
  await page.evaluate(() => {
    const grace = window.fake.addMember("Grace");
    const message = window.fake.receiveMessage(grace, "general", "Contract");
    window.fake.files.receive(grace, message, { name: "contract.md", type: "text/markdown", bytes: [35] });
  });
  await page.getByRole("button", { name: "Files", exact: true }).click();
  const row = page.getByRole("listitem").filter({ hasText: "contract.md" });
  const thumbnail = await row.getByText("MD", { exact: true }).locator("..").boundingBox();
  const name = await row.getByText("contract.md", { exact: true }).boundingBox();
  const meta = await row.getByText("Grace", { exact: true }).boundingBox();
  if (!thumbnail || !name || !meta) throw new Error("row parts not laid out");
  const textMiddle = (meta.y + name.y + name.height) / 2;
  expect(Math.abs(thumbnail.y + thumbnail.height / 2 - textMiddle)).toBeLessThanOrEqual(2);
});

test("the files view narrows to one channel, offering only places that have files", async ({ page }) => {
  await page.evaluate(() => {
    window.fake.handle("create_channel", { name: "random" });
    window.fake.handle("create_channel", { name: "quiet" });
    const grace = window.fake.addMember("Grace");
    window.fake.receivePrivateChannel(grace, "ops");
    const inGeneral = window.fake.receiveMessage(grace, "general", "Agenda");
    window.fake.files.receive(grace, inGeneral, { name: "agenda.txt", type: "text/plain", bytes: [1] });
    const inRandom = window.fake.receiveMessage(grace, "random", "Meme");
    window.fake.files.receive(grace, inRandom, { name: "meme.txt", type: "text/plain", bytes: [1] });
    const inOps = window.fake.receiveMessage(grace, "ops", "Runbook");
    window.fake.files.receive(grace, inOps, { name: "runbook.txt", type: "text/plain", bytes: [1] });
  });
  await page.getByRole("button", { name: "Files", exact: true }).click();
  const place = page.getByRole("combobox", { name: "In" });
  await expect(place).toHaveText("Anywhere");

  await place.click();
  await expect(page.getByRole("option")).toHaveText(["Anywhere", "ops", "random", "general"]);
  // The private channel's option says so.
  await expect(page.getByRole("option", { name: /ops/ }).getByLabel("Private")).toBeVisible();
  await page.getByRole("option", { name: "random" }).click();
  await expect(place).toHaveText("random");
  await expect(page.getByText("meme.txt")).toBeVisible();
  await expect(page.getByText("agenda.txt")).toHaveCount(0);
  await expect(page.getByText("runbook.txt")).toHaveCount(0);

  await place.click();
  await page.getByRole("option", { name: "Anywhere" }).click();
  await expect(page.getByText("agenda.txt")).toBeVisible();
  await expect(page.getByText("runbook.txt")).toBeVisible();
});

test("the kind filter sorts files the way their icons do", async ({ page }) => {
  await page.evaluate(() => {
    const grace = window.fake.addMember("Grace");
    const message = window.fake.receiveMessage(grace, "general", "Files");
    // Sent with a generic type: the name says what it is.
    window.fake.files.receive(grace, message, { name: "notes.md", type: "application/octet-stream", bytes: [35] });
    window.fake.files.receive(grace, message, { name: "budget.xlsx", type: "application/octet-stream", bytes: [1] });
    window.fake.files.receive(grace, message, { name: "backup.zip", type: "application/zip", bytes: [1] });
  });
  await page.getByRole("button", { name: "Files", exact: true }).click();
  await page.getByRole("radio", { name: "Documents" }).check();
  await expect(page.getByText("notes.md")).toBeVisible();
  await expect(page.getByText("budget.xlsx")).toBeVisible();
  await expect(page.getByText("backup.zip")).toHaveCount(0);
  await page.getByRole("radio", { name: "Other" }).check();
  await expect(page.getByText("backup.zip")).toBeVisible();
  await expect(page.getByText("notes.md")).toHaveCount(0);
});

test("the files view shows each day once, even for files shared out of time order", async ({ page }) => {
  const share = (name: string) =>
    page.evaluate((fileName) => {
      const grace = window.fake.memberNamed("Grace");
      const message = window.fake.receiveMessage(grace, "general", fileName);
      window.fake.files.receive(grace, message, { name: fileName, type: "text/plain", bytes: [1] });
    }, name);
  await page.clock.install({ time: new Date("2026-11-20T10:00:00") });
  await page.goto("");
  await page.evaluate(() => window.fake.addMember("Grace"));
  await share("morning.txt");
  await page.clock.setSystemTime(new Date("2026-11-22T09:00:00"));
  await share("today.txt");
  // A device whose clock is behind shares one dated two days ago, after today's.
  await page.clock.setSystemTime(new Date("2026-11-20T15:00:00"));
  await share("afternoon.txt");
  await page.clock.setSystemTime(new Date("2026-11-22T09:30:00"));
  await page.getByRole("button", { name: "Files", exact: true }).click();

  const friday = page.getByRole("region", { name: "Friday, November 20" });
  await expect(friday).toHaveCount(1);
  await expect(friday.getByRole("listitem")).toHaveText([/afternoon\.txt/, /morning\.txt/]);
  await expect(page.getByRole("region", { name: /^(Today|Friday, November 20)$/ })).toHaveText([
    /today\.txt/,
    /afternoon\.txt.*morning\.txt/,
  ]);
});

test("a file shared into a page shows in Files under its page, and leads back to it", async ({ page }) => {
  await page.evaluate(() => {
    const grace = window.fake.addMember("Grace");
    const plan = window.fake.seedPage("Launch plan", "");
    window.fake.files.receiveInPage(grace, plan, { name: "budget.csv", type: "text/csv", bytes: [49] });
  });
  await page.getByRole("button", { name: "Files", exact: true }).click();

  const row = page.getByRole("listitem").filter({ hasText: "budget.csv" });
  await expect(row).toContainText("Grace");
  await expect(row).toContainText("Launch plan");
  await page.getByRole("combobox", { name: "In" }).click();
  await page.getByRole("option", { name: "Launch plan" }).click();
  await expect(row).toBeVisible();
  await row.hover();
  await row.getByRole("button", { name: "Show budget.csv in its page" }).click();
  await expect(page.getByLabel("Page title")).toHaveValue("Launch plan");
});
