import { defaultKeymap, indentWithTab } from "@codemirror/commands";
import { markdown, markdownLanguage } from "@codemirror/lang-markdown";
import { HighlightStyle, syntaxHighlighting } from "@codemirror/language";
import { languages } from "@codemirror/language-data";
import { EditorState } from "@codemirror/state";
import { drawSelection, EditorView, highlightActiveLine, keymap, placeholder } from "@codemirror/view";
import { tags } from "@lezer/highlight";
import { useEffect, useRef, useState } from "react";
import { yCollab } from "y-codemirror.next";
import * as Y from "yjs";
import { api, onChange } from "../../lib/api";
import { reportError } from "../../lib/toast";
import { PAGE_TEXT } from "./pageText";

/** Local keystrokes are batched before being sealed and sent. */
const SEND_DELAY_MS = 120;
const REMOTE = "remote";

const markdownStyle = HighlightStyle.define([
  { tag: tags.heading1, fontSize: "1.75em", fontWeight: "700", letterSpacing: "-0.02em" },
  { tag: tags.heading2, fontSize: "1.4em", fontWeight: "650", letterSpacing: "-0.01em" },
  { tag: tags.heading3, fontSize: "1.18em", fontWeight: "620" },
  { tag: [tags.heading4, tags.heading5, tags.heading6], fontWeight: "600" },
  { tag: tags.strong, fontWeight: "650" },
  { tag: tags.emphasis, fontStyle: "italic" },
  { tag: tags.strikethrough, textDecoration: "line-through", color: "var(--text-muted)" },
  { tag: tags.link, color: "var(--accent-text)", textDecoration: "underline" },
  { tag: tags.url, color: "var(--text-faint)" },
  { tag: tags.monospace, fontFamily: "var(--font-mono)", fontSize: "0.88em", color: "var(--accent-text)" },
  { tag: tags.quote, color: "var(--text-muted)", fontStyle: "italic" },
  { tag: [tags.processingInstruction, tags.meta, tags.contentSeparator], color: "var(--text-faint)" },
  { tag: tags.list, color: "var(--accent)" },
]);

const theme = EditorView.theme({
  "&": { fontSize: "15.5px", backgroundColor: "transparent", color: "var(--text)" },
  "&.cm-focused": { outline: "none" },
  ".cm-scroller": { fontFamily: "var(--font-sans)", lineHeight: "1.7", overflow: "visible" },
  ".cm-content": { padding: "0 0 40vh", caretColor: "var(--accent)" },
  ".cm-line": { padding: "0" },
  ".cm-activeLine": { backgroundColor: "transparent" },
  ".cm-cursor": { borderLeftColor: "var(--accent)", borderLeftWidth: "2px" },
  "&.cm-focused .cm-selectionBackground, .cm-selectionBackground": {
    backgroundColor: "color-mix(in srgb, var(--accent) 22%, transparent) !important",
  },
  ".cm-placeholder": { color: "var(--text-faint)" },
});

/**
 * Collaborative markdown editor. The Y.Doc mirrors the page held by the core:
 * local edits go out as Yjs updates, remote ones come back through change
 * events. Applying an update twice is harmless, so no echo tracking is needed.
 */
export function Editor({ pageId }: { pageId: string }) {
  // A Yjs doc can't forget what it merged, so a rebuilt page gets a fresh one.
  const [resets, setResets] = useState(0);
  useEffect(
    () =>
      onChange((change) => {
        if (change.kind === "page_reset" && change.page === pageId) setResets((count) => count + 1);
      }),
    [pageId],
  );
  return <PageEditor key={resets} pageId={pageId} />;
}

function PageEditor({ pageId }: { pageId: string }) {
  const host = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const doc = new Y.Doc();
    const text = doc.getText(PAGE_TEXT);
    let pending: Uint8Array[] = [];
    let timer: ReturnType<typeof setTimeout> | undefined;
    const flush = () => {
      timer = undefined;
      if (pending.length === 0) return;
      const update = Y.mergeUpdates(pending);
      pending = [];
      api.applyDocUpdate(pageId, update).catch(reportError);
    };
    doc.on("update", (update: Uint8Array, origin: unknown) => {
      if (origin === REMOTE) return;
      pending.push(update);
      timer ??= setTimeout(flush, SEND_DELAY_MS);
    });

    // Subscribe before loading so nothing that arrives in between is lost.
    const unsubscribe = onChange((change) => {
      if (change.kind === "doc" && change.page === pageId) Y.applyUpdate(doc, Uint8Array.from(change.update), REMOTE);
    });

    let view: EditorView | undefined;
    let disposed = false;
    api.pageState(pageId).then((state) => {
      if (disposed || !host.current) return;
      if (state.length > 0) Y.applyUpdate(doc, state, REMOTE);
      const undoManager = new Y.UndoManager(text);
      view = new EditorView({
        parent: host.current,
        state: EditorState.create({
          doc: text.toString(),
          extensions: [
            drawSelection(),
            highlightActiveLine(),
            EditorView.lineWrapping,
            markdown({ base: markdownLanguage, codeLanguages: languages }),
            syntaxHighlighting(markdownStyle),
            placeholder("Start writing… Markdown works, and everyone here edits together."),
            keymap.of([indentWithTab, ...defaultKeymap]),
            // Yjs undo only reverts your own edits, never a teammate's.
            yCollab(text, null, { undoManager }),
            theme,
          ],
        }),
      });
      if (text.length === 0) view.focus();
    }, reportError);

    return () => {
      disposed = true;
      unsubscribe();
      if (timer) clearTimeout(timer);
      flush();
      view?.destroy();
      doc.destroy();
    };
  }, [pageId]);

  return <div ref={host} className="selectable min-h-[50vh]" />;
}
