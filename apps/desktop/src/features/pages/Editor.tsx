import { defaultKeymap, indentWithTab } from "@codemirror/commands";
import { markdown, markdownLanguage } from "@codemirror/lang-markdown";
import { languages } from "@codemirror/language-data";
import { EditorState } from "@codemirror/state";
import { drawSelection, EditorView, keymap, placeholder } from "@codemirror/view";
import { useEffect, useRef, useState } from "react";
import { yCollab, yUndoManagerKeymap } from "y-codemirror.next";
import * as Y from "yjs";
import { api, onChange } from "../../lib/api";
import { reportError } from "../../lib/toast";
import { useWorkspace } from "../../lib/workspace";
import { formattingKeymap } from "./editor/commands";
import { livePreview, setPageTitles } from "./editor/livePreview";
import { type MenuKey, type Suggestion, suggestions } from "./editor/suggestions";
import { pageTheme } from "./editor/theme";
import { PAGE_TEXT } from "./pageText";
import { SuggestionMenu } from "./SuggestionMenu";

/** Local keystrokes are batched before being sealed and sent. */
const SEND_DELAY_MS = 120;
const REMOTE = "remote";

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
  const [view, setView] = useState<EditorView | null>(null);
  const { pages, navigate } = useWorkspace();
  const navigateRef = useRef(navigate);
  navigateRef.current = navigate;
  const [suggestion, setSuggestion] = useState<Suggestion | null>(null);
  const menuKeys = useRef<(key: MenuKey, view: EditorView) => boolean>(() => false);

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

    let created: EditorView | undefined;
    let disposed = false;
    api.pageState(pageId).then((state) => {
      if (disposed || !host.current) return;
      if (state.length > 0) Y.applyUpdate(doc, state, REMOTE);
      const undoManager = new Y.UndoManager(text);
      created = new EditorView({
        parent: host.current,
        state: EditorState.create({
          doc: text.toString(),
          extensions: [
            drawSelection(),
            EditorView.lineWrapping,
            markdown({ base: markdownLanguage, codeLanguages: languages }),
            livePreview({ onOpenPage: (id) => navigateRef.current({ kind: "page", id }) }),
            suggestions({ onKey: (key, editor) => menuKeys.current(key, editor), onChange: setSuggestion }),
            placeholder("Type / for blocks, @ to mention someone, [[ to link a page"),
            keymap.of([
              ...formattingKeymap,
              ...yUndoManagerKeymap,
              indentWithTab,
              // Ctrl+/ stays the app's shortcut sheet; HTML comments have no place in a page.
              ...defaultKeymap.filter((binding) => binding.key !== "Mod-/"),
            ]),
            // Yjs undo only reverts your own edits, never a teammate's.
            yCollab(text, null, { undoManager }),
            pageTheme,
          ],
        }),
      });
      if (text.length === 0) created.focus();
      setView(created);
    }, reportError);

    return () => {
      disposed = true;
      unsubscribe();
      if (timer) clearTimeout(timer);
      flush();
      created?.destroy();
      doc.destroy();
    };
  }, [pageId]);

  useEffect(() => {
    view?.dispatch({ effects: setPageTitles.of(new Map(pages.map((page) => [page.id, page.title]))) });
  }, [view, pages]);

  return (
    <div className="relative">
      <div ref={host} className="selectable min-h-[50vh]" />
      <SuggestionMenu view={view} suggestion={suggestion} pageId={pageId} keys={menuKeys} />
    </div>
  );
}
