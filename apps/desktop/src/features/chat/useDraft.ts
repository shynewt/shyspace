import { useEffect, useRef, useState } from "react";
import { api } from "../../lib/api";
import { saveBeforeSwitch } from "../../lib/beforeSwitch";
import { reportError } from "../../lib/toast";

const SAVE_DELAY_MS = 400;

/**
 * Keeps the composer's unsent text as a draft for `target` (a channel or a
 * thread's first message): restores it on open, saves after a pause, and
 * saves at once when the composer closes. Only edits are saved.
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
