import { expect, type Page } from "@playwright/test";

/**
 * Opens a submenu and picks a choice, moving the pointer in steps as a person
 * does. A one-jump move toward a submenu that opened on the left reads as
 * leaving the menu, and closes it.
 */
export async function chooseInSubmenu(page: Page, submenu: string, choice: string) {
  await page.getByRole("menuitem", { name: submenu }).hover();
  const item = page.getByRole("menuitemradio", { name: choice });
  await expect(item).toBeVisible();
  const box = await item.boundingBox();
  if (!box) throw new Error(`no box for ${choice}`);
  await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2, { steps: 12 });
  await item.click();
}
