import type { Draft } from "../../../src/lib/api";

type Stored = { text: string; updated_ms: number };

/**
 * Drafts, like the core keeps them: personal, newest first, blank removes.
 * Unsaved edits of your messages are kept apart, per conversation and
 * message, and stay (marked deleted) when their message goes.
 */
export class FakeDrafts {
  private drafts = new Map<string, Stored>();
  private edits = new Map<string, Stored & { conversation: string; message: string }>();

  set(target: string, text: string): void {
    if (text.trim()) this.drafts.set(target, { text, updated_ms: Date.now() });
    else this.drafts.delete(target);
  }

  /** The draft for `target`; empty if there is none. */
  get(target: string): string {
    return this.drafts.get(target)?.text ?? "";
  }

  setEdit(conversation: string, message: string, text: string): void {
    const key = `${conversation}:${message}`;
    if (text.trim()) this.edits.set(key, { conversation, message, text, updated_ms: Date.now() });
    else this.edits.delete(key);
  }

  /** The unsaved edit of `message`; empty if there is none. */
  getEdit(conversation: string, message: string): string {
    return this.edits.get(`${conversation}:${message}`)?.text ?? "";
  }

  /** Moves a deleted message's edit into the conversation's draft, after any draft there. */
  keepEditOfDeleted(conversation: string, message: string): void {
    const edit = this.getEdit(conversation, message);
    if (!edit) return;
    const draft = this.get(conversation);
    this.set(conversation, draft.trim() ? `${draft}\n\n${edit}` : edit);
    this.setEdit(conversation, message, "");
  }

  list(channelOf: (target: string) => string | null, isMessage: (id: string) => boolean): Draft[] {
    const drafts = [...this.drafts].map(([target, draft]) => ({ target, ...draft, editing: null }));
    const edits = [...this.edits.values()].map(({ conversation, message, ...draft }) => ({
      target: conversation,
      ...draft,
      editing: { message, is_deleted: !isMessage(message) },
    }));
    return [...drafts, ...edits]
      .flatMap((draft) => {
        const channel = channelOf(draft.target);
        return channel ? [{ ...draft, channel }] : [];
      })
      .sort((left, right) => right.updated_ms - left.updated_ms);
  }
}
