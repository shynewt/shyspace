import { expect, type Page } from "@playwright/test";
import type { Change } from "../../src/lib/api";

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

declare global {
  interface Window {
    openedLinks?: string[];
    /** Tauri's bridge; `mockIPC` routes it to the fake. */
    __TAURI_INTERNALS__: { invoke: (command: string, args?: unknown) => Promise<unknown> };
  }
}

/** Records the links the app asks the system to open, instead of opening them. */
export async function recordOpenedLinks(page: Page) {
  await page.evaluate(() => {
    const opened: string[] = [];
    window.openedLinks = opened;
    const answer = window.fake.handle.bind(window.fake);
    window.fake.handle = (command, args) => {
      if (command !== "plugin:opener|open_url") return answer(command, args);
      if (args && "url" in args && typeof args.url === "string") opened.push(args.url);
      return null;
    };
  });
}

export function openedLinks(page: Page): Promise<string[]> {
  return page.evaluate(() => window.openedLinks ?? []);
}

/** From now on answers `command` with `value`, as the core would in another state, and announces `change`. */
export async function answerWith(page: Page, command: string, value: unknown, change: Change) {
  await page.evaluate(
    ({ command, value, change }) => {
      const answer = window.fake.handle.bind(window.fake);
      window.fake.handle = (asked, args) => (asked === command ? structuredClone(value) : answer(asked, args));
      return window.__TAURI_INTERNALS__.invoke("plugin:event|emit", { event: "change", payload: change });
    },
    { command, value, change },
  );
}
