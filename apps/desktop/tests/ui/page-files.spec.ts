import { expect, type Page, test } from "@playwright/test";
import { TINY_PNG } from "./fake/files";

const lines = (page: Page) => page.locator(".cm-line");

async function openPage(page: Page, title: string, text: string): Promise<string> {
  await page.goto("");
  const id = await page.evaluate(([title, text]) => window.fake.seedPage(title, text), [title, text]);
  await page.getByRole("navigation", { name: "Workspace" }).getByRole("button", { name: title, exact: true }).click();
  await expect(page.getByLabel("Page title")).toHaveValue(title);
  return id;
}

/** Pastes files into the focused editor, as the system clipboard would. */
async function paste(page: Page, files: { name: string; type: string; bytes: number[] }[]) {
  await page.evaluate((files) => {
    const transfer = new DataTransfer();
    for (const file of files)
      transfer.items.add(new File([new Uint8Array(file.bytes)], file.name, { type: file.type }));
    document.activeElement?.dispatchEvent(
      new ClipboardEvent("paste", { clipboardData: transfer, bubbles: true, cancelable: true }),
    );
  }, files);
}

function pageFiles(page: Page, pageId: string) {
  return page.evaluate(
    (id) => window.fake.handle("page_files", { page: id }) as { id: string; name: string }[],
    pageId,
  );
}

test("a pasted image uploads into the page and is linked where it was pasted", async ({ page }) => {
  const pageId = await openPage(page, "Plan", "Intro");
  await lines(page).first().click();
  await page.keyboard.press("End");
  await page.keyboard.press("Enter");
  await page.evaluate(() => {
    window.fake.files.holdNextShare = true;
  });
  await paste(page, [{ name: "image.png", type: "image/png", bytes: TINY_PNG }]);

  await expect(page.getByText("Uploading Plan image 1.png…")).toBeVisible();
  await page.evaluate(() => window.fake.files.releaseShare?.());
  await expect(page.getByText("Uploading Plan image 1.png…")).toHaveCount(0);
  const [file] = await pageFiles(page, pageId);
  expect(file.name).toBe("Plan image 1.png");
  await expect.poll(() => page.evaluate(() => window.fake.pageText("Plan"))).toBe(`Intro\n![](shy://file/${file.id})`);
});

test("an image the web view left out of a paste is read from the clipboard into the page", async ({ page }) => {
  const pageId = await openPage(page, "Plan", "");
  await lines(page).first().click();
  await page.evaluate(() => {
    window.fake.files.clipboardImage = { width: 1, height: 1, rgba: [255, 0, 0, 255] };
  });
  await paste(page, []);

  await expect.poll(async () => (await pageFiles(page, pageId)).length).toBe(1);
  const [file] = await pageFiles(page, pageId);
  expect(file.name).toBe("Plan image 1.png");
  await expect.poll(() => page.evaluate(() => window.fake.pageText("Plan"))).toBe(`![](shy://file/${file.id})`);
});

test("a pasted file that isn't an image is linked by its name", async ({ page }) => {
  const pageId = await openPage(page, "Plan", "");
  await lines(page).first().click();
  await paste(page, [{ name: "budget [draft].csv", type: "text/csv", bytes: [49] }]);

  await expect.poll(async () => (await pageFiles(page, pageId)).length).toBe(1);
  const [file] = await pageFiles(page, pageId);
  await expect
    .poll(() => page.evaluate(() => window.fake.pageText("Plan")))
    .toBe(`[budget \\[draft\\].csv](shy://file/${file.id})`);
});

test("a failed upload into a page inserts nothing and says why", async ({ page }) => {
  await openPage(page, "Plan", "Intro");
  await lines(page).first().click();
  await page.evaluate(() => {
    window.fake.files.failNextSend = true;
  });
  await paste(page, [{ name: "photo.png", type: "image/png", bytes: TINY_PNG }]);

  await expect(page.getByText("couldn't reach the bucket")).toBeVisible();
  await expect(page.getByText(/Uploading/)).toHaveCount(0);
  expect(await page.evaluate(() => window.fake.pageText("Plan"))).toBe("Intro");
});

test("files dropped on the window while a page is open land at its cursor", async ({ page }) => {
  const pageId = await openPage(page, "Plan", "Intro");
  await lines(page).first().click();
  await page.keyboard.press("End");
  await page.evaluate(() => window.fake.files.drop([{ name: "notes.pdf", type: "application/pdf", bytes: [37] }]));

  await expect.poll(async () => (await pageFiles(page, pageId)).length).toBe(1);
  const [file] = await pageFiles(page, pageId);
  await expect
    .poll(() => page.evaluate(() => window.fake.pageText("Plan")))
    .toBe(`Intro\n[notes.pdf](shy://file/${file.id})`);
});

/** A page whose text is `text(fileId)` around one PNG of its own, shared by Grace. */
async function pageWithImage(page: Page, text: (fileId: string) => string): Promise<string> {
  await page.goto("");
  const fileId = await page.evaluate(
    ([png, template]) => {
      const pageId = window.fake.seedPage("Plan", "");
      const grace = window.fake.addMember("Grace");
      const id = window.fake.files.receiveInPage(grace, pageId, { name: "team.png", type: "image/png", bytes: png });
      window.fake.receivePageEdit("Plan", { at: 0, insert: template.replace("FILE", id) });
      return id;
    },
    [TINY_PNG, text("FILE")] as const,
  );
  await page.getByRole("navigation", { name: "Workspace" }).getByRole("button", { name: "Plan", exact: true }).click();
  await expect(page.getByLabel("Page title")).toHaveValue("Plan");
  return fileId;
}

/** Selects the image on line 2 with the keyboard, as Shift+arrow over it does. */
async function selectImageOnSecondLine(page: Page) {
  await lines(page).first().click();
  await page.keyboard.press("End");
  await page.keyboard.press("ArrowDown");
  await page.keyboard.press("Home");
  await page.keyboard.press("Shift+End");
}

test("an image in a page shows with its caption and opens large", async ({ page }) => {
  await pageWithImage(page, (id) => `Intro\n![Team photo](shy://file/${id})`);

  const image = page.locator(".cm-content").getByRole("img", { name: "Team photo" });
  await expect(image).toBeVisible();
  await expect(page.locator(".cm-content").getByRole("button", { name: "Team photo" })).toBeVisible();
  await image.click();
  await expect(page.getByRole("dialog")).toBeVisible();
});

test("an image of a file from elsewhere shows as unavailable, never loaded", async ({ page }) => {
  await page.goto("");
  await page.evaluate((png) => {
    const grace = window.fake.addMember("Grace");
    const message = window.fake.receiveMessage(grace, "general", "photo");
    const chatFile = window.fake.files.receive(grace, message, { name: "x.png", type: "image/png", bytes: png });
    window.fake.seedPage("Plan", `![Stolen](shy://file/${chatFile})`);
  }, TINY_PNG);
  await page.getByRole("navigation", { name: "Workspace" }).getByRole("button", { name: "Plan", exact: true }).click();

  await expect(page.locator(".cm-content").getByText("Image unavailable")).toBeVisible();
  await expect(page.locator(".cm-content").getByRole("img")).toHaveCount(0);
});

test("Alt+arrows resize a selected image by tens, and Alt+0 takes the width away", async ({ page }) => {
  const fileId = await pageWithImage(page, (id) => `Intro\n![Team photo](shy://file/${id})`);
  await expect(page.locator(".cm-content").getByRole("img", { name: "Team photo" })).toBeVisible();
  await selectImageOnSecondLine(page);

  await page.keyboard.press("Alt+ArrowLeft");
  await page.keyboard.press("Alt+ArrowLeft");
  await expect
    .poll(() => page.evaluate(() => window.fake.pageText("Plan")))
    .toBe(`Intro\n![Team photo](shy://file/${fileId}#width=80)`);
  await page.keyboard.press("Alt+ArrowRight");
  await expect.poll(() => page.evaluate(() => window.fake.pageText("Plan"))).toContain("#width=90)");
  await page.keyboard.press("Alt+0");
  await expect
    .poll(() => page.evaluate(() => window.fake.pageText("Plan")))
    .toBe(`Intro\n![Team photo](shy://file/${fileId})`);
});

test("a caption is edited in place and written back escaped", async ({ page }) => {
  const fileId = await pageWithImage(page, (id) => `Intro\n![](shy://file/${id})`);
  await expect(page.locator(".cm-content").getByRole("img")).toBeVisible();
  await selectImageOnSecondLine(page);

  await page.keyboard.press("Enter");
  const caption = page.getByRole("textbox", { name: "Caption" });
  await expect(caption).toBeFocused();
  await caption.fill("Our [team]");
  await caption.press("Enter");
  await expect
    .poll(() => page.evaluate(() => window.fake.pageText("Plan")))
    .toBe(`Intro\n![Our \\[team\\]](shy://file/${fileId})`);
  await expect(page.locator(".cm-content").getByRole("button", { name: "Our [team]" })).toBeVisible();
});

test("dragging an image's handle sets its width", async ({ page }) => {
  await pageWithImage(page, (id) => `Intro\n![Team photo](shy://file/${id}#width=80)`);
  const image = page.locator(".cm-content").getByRole("img", { name: "Team photo" });
  await expect(image).toBeVisible();
  await image.hover();
  const handle = page.locator(".cm-image-handle[data-side=right]");
  const box = await handle.boundingBox();
  if (!box) throw new Error("the handle isn't on screen");
  await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
  await page.mouse.down();
  await page.mouse.move(box.x - 60, box.y + box.height / 2, { steps: 4 });
  await page.mouse.up();

  await expect
    .poll(async () => Number((await page.evaluate(() => window.fake.pageText("Plan"))).match(/#width=(\d+)/)?.[1]))
    .toBeLessThan(80);
});

test("a file in a page is a chip with its current name that opens the viewer", async ({ page }) => {
  await page.goto("");
  await page.evaluate(() => {
    const pageId = window.fake.seedPage("Plan", "");
    const grace = window.fake.addMember("Grace");
    const id = window.fake.files.receiveInPage(grace, pageId, { name: "q3.csv", type: "text/csv", bytes: [49] });
    window.fake.receivePageEdit("Plan", { at: 0, insert: `Numbers: [q3.csv](shy://file/${id})` });
  });
  await page.getByRole("navigation", { name: "Workspace" }).getByRole("button", { name: "Plan", exact: true }).click();

  const chip = page.locator(".cm-content").getByRole("link", { name: "q3.csv" });
  await expect(chip).toBeVisible();
  await chip.click();
  await expect(page.getByRole("dialog")).toBeVisible();
});

test("a link to a file from elsewhere is an unavailable chip that opens nothing", async ({ page }) => {
  await page.goto("");
  await page.evaluate(() => {
    const grace = window.fake.addMember("Grace");
    const message = window.fake.receiveMessage(grace, "general", "numbers");
    const chatFile = window.fake.files.receive(grace, message, { name: "q3.csv", type: "text/csv", bytes: [49] });
    window.fake.seedPage("Plan", `Numbers: [q3.csv](shy://file/${chatFile})`);
  });
  await page.getByRole("navigation", { name: "Workspace" }).getByRole("button", { name: "Plan", exact: true }).click();

  await expect(lines(page).first()).toHaveText("Numbers: File unavailable");
  await expect(page.locator(".cm-content").getByRole("link")).toHaveCount(0);
});

test("an image without a caption offers one only while selected", async ({ page }) => {
  await pageWithImage(page, (id) => `Intro\n![](shy://file/${id})`);
  const image = page.locator(".cm-content").getByRole("img");
  await expect(image).toBeVisible();
  const addCaption = page.locator(".cm-content").getByRole("button", { name: "Add a caption" });
  await image.hover();
  await expect(addCaption).toHaveCount(0);

  await selectImageOnSecondLine(page);
  await expect(addCaption).toBeVisible();
  await page.keyboard.press("ArrowRight");
  await expect(addCaption).toHaveCount(0);
});

test("a selected image shows its ring, not the text selection's tint or caret", async ({ page }) => {
  await pageWithImage(page, (id) => `Intro\n![Team photo](shy://file/${id})`);
  await expect(page.locator(".cm-content").getByRole("img", { name: "Team photo" })).toBeVisible();
  await selectImageOnSecondLine(page);

  const frame = page.locator("[data-image-selected]");
  await expect(frame).toHaveCount(1);
  await expect(page.locator(".cm-selectionBackground").first()).toBeHidden();
  await expect(page.locator(".cm-cursor").first()).toBeHidden();

  // A selection running from the text above through the image keeps the text's tint.
  await lines(page).first().click();
  await page.keyboard.press("Home");
  await page.keyboard.press("Shift+ArrowDown");
  await page.keyboard.press("Shift+End");
  await expect(frame).toHaveCount(1);
  await expect(page.locator(".cm-selectionBackground").first()).toBeVisible();
});
