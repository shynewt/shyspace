import { type Ref, useEffect, useImperativeHandle, useLayoutEffect, useRef, useState } from "react";
import { api } from "../../lib/api";
import { MAX_PAGE_TITLE_CHARS } from "../../lib/limits";
import { reportError } from "../../lib/toast";

/**
 * The page's title: grows to fit long titles, saves on blur or Enter, and
 * follows remote renames while not being edited. An untitled page starts
 * here, so a new page is named first; Enter moves on to the body.
 */
export function PageTitle({ pageId, title, ref }: { pageId: string; title: string; ref: Ref<{ focus: () => void }> }) {
  const [draft, setDraft] = useState(title);
  const [isEditing, setEditing] = useState(false);
  const field = useRef<HTMLTextAreaElement>(null);
  const isUntitledAtStart = useRef(title === "");
  useImperativeHandle(ref, () => ({ focus: () => field.current?.focus() }), []);

  useEffect(() => {
    if (!isEditing) setDraft(title);
  }, [title, isEditing]);

  useEffect(() => {
    if (isUntitledAtStart.current) field.current?.focus();
  }, []);

  // Fit after every render (the text may have changed) and whenever the width does.
  useLayoutEffect(() => {
    if (field.current) fitHeight(field.current);
  });
  useEffect(() => {
    const element = field.current;
    if (!element) return;
    const observer = new ResizeObserver(() => fitHeight(element));
    observer.observe(element);
    return () => observer.disconnect();
  }, []);

  const save = () => {
    setEditing(false);
    if (draft.trim() !== title) api.renamePage(pageId, draft).catch(reportError);
  };

  return (
    <textarea
      ref={field}
      aria-label="Page title"
      rows={1}
      value={draft}
      onFocus={() => setEditing(true)}
      onChange={(event) => {
        const oneLine = event.target.value.replace(/\r?\n/g, " ");
        setDraft(Array.from(oneLine).slice(0, MAX_PAGE_TITLE_CHARS).join(""));
      }}
      onBlur={save}
      onKeyDown={(event) => {
        if (event.key !== "Enter") return;
        event.preventDefault();
        event.currentTarget.closest("article")?.querySelector<HTMLElement>(".cm-content")?.focus();
      }}
      placeholder="Untitled"
      className="block w-full resize-none overflow-hidden bg-transparent text-3xl leading-tight font-bold tracking-tight outline-none placeholder:text-muted"
    />
  );
}

function fitHeight(element: HTMLTextAreaElement) {
  element.style.height = "auto";
  element.style.height = `${element.scrollHeight}px`;
}
