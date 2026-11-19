import type { LaterItem, LinkedMessage, Page } from "../../../src/lib/api";

type Saved = { saved_ms: number; remind_ms: number | null; is_reminded: boolean; is_done: boolean; order: number };

/** Later, like the core keeps it: personal, newest first, reminders fire once. */
export class FakeLater {
  private saved = new Map<string, Saved>();
  private saves = 0;

  has(item: string): boolean {
    return this.saved.has(item);
  }

  save(item: string, remindMs: number | null): void {
    const existing = this.saved.get(item);
    this.saves += 1;
    this.saved.set(item, {
      saved_ms: existing?.saved_ms ?? Date.now(),
      remind_ms: remindMs,
      is_reminded: false,
      is_done: false,
      order: existing?.order ?? this.saves,
    });
  }

  remove(item: string): void {
    this.saved.delete(item);
  }

  setDone(item: string, isDone: boolean): void {
    const entry = this.saved.get(item);
    if (entry) entry.is_done = isDone;
  }

  list(resolve: (item: string) => { message: LinkedMessage | null; page: Page | null }): LaterItem[] {
    return [...this.saved]
      .sort(([, left], [, right]) => right.order - left.order)
      .flatMap(([id, entry]) => {
        const { message, page } = resolve(id);
        if (!message && !page) return [];
        return [{ id, saved_ms: entry.saved_ms, remind_ms: entry.remind_ms, is_done: entry.is_done, message, page }];
      });
  }

  takeDue(now: number): string[] {
    const due = [...this.saved].filter(
      ([, entry]) => entry.remind_ms !== null && entry.remind_ms <= now && !entry.is_reminded && !entry.is_done,
    );
    for (const [, entry] of due) entry.is_reminded = true;
    return due.map(([id]) => id);
  }
}
