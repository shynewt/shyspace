import { useCallback, useEffect, useRef } from "react";
import { api, type Message } from "../../lib/api";
import { saveBeforeSwitch } from "../../lib/beforeSwitch";
import { reportError, showToast } from "../../lib/toast";

const SAVE_DELAY_MS = 400;

type Session = { messageId: string; saved: string; isStored: boolean };

/**
 * Keeps an unsaved edit in the composer for `conversation` (a channel, or a
 * thread's first message) as a draft of its own, apart from the
 * conversation's draft: saved after a pause and when the composer closes,
 * once the text differs from where editing started. When the conversation
 * opens again (`isReady` once its draft is back), the edit resumes through
 * `onResume`; if its message was deleted meanwhile, its text joins the
 * conversation's draft (which the open composer then shows), so nothing is
 * lost.
 */
export function useEditDraft({
  conversation,
  text,
  isReady,
  onResume,
}: {
  conversation: string;
  text: string;
  isReady: boolean;
  onResume: (message: Message, text: string) => void;
}) {
  const session = useRef<Session | null>(null);
  const latest = useRef({ text, onResume });
  useEffect(() => {
    latest.current = { text, onResume };
  }, [text, onResume]);

  useEffect(() => {
    if (!isReady) return;
    let isCurrent = true;
    resumeEdit(conversation, () => isCurrent, latest).catch(reportError);
    return () => {
      isCurrent = false;
    };
  }, [conversation, isReady]);

  const save = useCallback(() => {
    const current = session.current;
    const pending = latest.current.text;
    if (!current || pending === current.saved) return Promise.resolve();
    current.saved = pending;
    current.isStored = pending.trim() !== "";
    return api.setEditDraft(conversation, current.messageId, pending).catch(reportError);
  }, [conversation]);

  useEffect(() => {
    if (!session.current || text === session.current.saved) return;
    const timer = setTimeout(save, SAVE_DELAY_MS);
    return () => clearTimeout(timer);
  }, [text, save]);
  useEffect(() => () => void save(), [save]);
  useEffect(() => saveBeforeSwitch(save), [save]);

  /** Stops keeping the current edit, removing what was saved of it (saved or cancelled). */
  const forget = useCallback(() => {
    const current = session.current;
    session.current = null;
    if (current?.isStored) api.setEditDraft(conversation, current.messageId, "").catch(reportError);
  }, [conversation]);

  /** Starts keeping the edit of `messageId`, which begins at `startText`; `isStored` when it was resumed. */
  const begin = useCallback(
    (messageId: string, startText: string, isStored: boolean) => {
      // Going straight from one edit to another drops the first, as the composer does.
      if (session.current && session.current.messageId !== messageId) forget();
      session.current = { messageId, saved: startText, isStored };
    },
    [forget],
  );

  return { begin, forget };
}

async function resumeEdit(
  conversation: string,
  isCurrent: () => boolean,
  latest: { current: { onResume: (message: Message, text: string) => void } },
) {
  const edit = (await api.drafts()).find((draft) => draft.target === conversation && draft.editing);
  if (!edit?.editing || !isCurrent()) return;
  const linked = edit.editing.is_deleted ? null : await api.linkedMessage(edit.editing.message);
  if (!isCurrent()) return;
  if (linked) return latest.current.onResume(linked.message, edit.text);
  await api.keepEditOfDeletedMessage(conversation, edit.editing.message);
  showToast("The message you were editing was deleted, so your changes are in the message box.", "info");
}
