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
