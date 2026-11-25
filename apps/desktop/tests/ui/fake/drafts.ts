import type { Draft } from "../../../src/lib/api";

/** Drafts, like the core keeps them: personal, newest first, blank removes. */
export class FakeDrafts {
  private drafts = new Map<string, { text: string; updated_ms: number }>();

  set(target: string, text: string): void {
    if (text.trim()) this.drafts.set(target, { text, updated_ms: Date.now() });
    else this.drafts.delete(target);
  }

  /** The draft for `target`; empty if there is none. */
  get(target: string): string {
    return this.drafts.get(target)?.text ?? "";
  }

  list(channelOf: (target: string) => string | null): Draft[] {
    return [...this.drafts]
      .flatMap(([target, draft]) => {
        const channel = channelOf(target);
        return channel ? [{ target, channel, ...draft }] : [];
      })
      .sort((left, right) => right.updated_ms - left.updated_ms);
  }
}
