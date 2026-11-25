import { useEffect, useRef, useState } from "react";
import { api, onChange } from "../../lib/api";
import { saveBeforeSwitch } from "../../lib/beforeSwitch";
import { reportError } from "../../lib/toast";

const SAVE_DELAY_MS = 400;

/**
 * Keeps the composer's unsent text as a draft for `target` (a channel or a
 * thread's first message): restores it on open, saves after a pause, and
 * saves at once when the composer closes. Only edits are saved. A draft
 * stored from elsewhere while it's open (a reply kept from a notification)
 * shows at once, after anything typed and not yet saved.
 */
export function useDraft(target: string, text: string, onRestore: (text: string) => void) {
  const latest = useRef({ text, onRestore });
  const isDirty = useRef(false);
  const [isRestored, setRestored] = useState(false);
  // The text as last saved or restored; only a change from it is an edit.
  const saved = useRef("");

  useEffect(() => {
    latest.current = { text, onRestore };
  }, [text, onRestore]);

  useEffect(() => {
    let isCurrent = true;
    api.drafts().then((drafts) => {
      if (!isCurrent) return;
      const draft = drafts.find((candidate) => candidate.target === target);
      // Never replace what the user already started typing.
      if (draft && !latest.current.text) latest.current.onRestore(draft.text);
      // What's stored, not what's shown: text typed before this answer is an edit.
      saved.current = draft?.text ?? "";
      setRestored(true);
    }, reportError);
    return () => {
      isCurrent = false;
    };
  }, [target]);

  useEffect(() => {
    if (!isRestored) return;
    // Changes can come in quick succession (a send clears a draft, then
    // it's put back); only the newest read counts.
    let reads = 0;
    return onChange((change) => {
      if (change.kind !== "drafts") return;
      const read = ++reads;
      api.drafts().then((drafts) => {
        if (read !== reads) return;
        const stored = drafts.find((candidate) => candidate.target === target)?.text ?? "";
        // Our own save, or nothing new.
        if (stored === saved.current) return;
        const typed = latest.current.text;
        const shown = typed === saved.current ? stored : withAdded(typed, saved.current, stored);
        saved.current = stored;
        // At once, not on the next render, so a change right behind this one sees it.
        latest.current.text = shown;
        latest.current.onRestore(shown);
      }, reportError);
    });
  }, [target, isRestored]);

  useEffect(() => {
    if (!isRestored) return;
    if (text === saved.current) return;
    isDirty.current = true;
    const timer = setTimeout(() => {
      // Already saved, before switching workspace.
      if (!isDirty.current) return;
      saved.current = text;
      isDirty.current = false;
      api.setDraft(target, text).catch(reportError);
    }, SAVE_DELAY_MS);
    return () => clearTimeout(timer);
  }, [target, text, isRestored]);

  useEffect(
    () => () => {
      if (isDirty.current) api.setDraft(target, latest.current.text).catch(reportError);
    },
    [target],
  );

  useEffect(
    () =>
      saveBeforeSwitch(async () => {
        if (!isDirty.current) return;
        isDirty.current = false;
        saved.current = latest.current.text;
        await api.setDraft(target, latest.current.text).catch(reportError);
      }),
    [target],
  );
}

/**
 * What's typed but unsaved, with what was stored meanwhile: the part added
 * after the draft the typing started from, or all of it if it was replaced.
 */
function withAdded(typed: string, before: string, stored: string): string {
  const added = (stored.startsWith(before) ? stored.slice(before.length) : stored).replace(/^\n+/, "");
  return [typed, added].filter(Boolean).join("\n\n");
}
