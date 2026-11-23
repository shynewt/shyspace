import { defaultKeymap, indentWithTab } from "@codemirror/commands";
import { markdown, markdownLanguage } from "@codemirror/lang-markdown";
import { languages } from "@codemirror/language-data";
import { EditorState } from "@codemirror/state";
import { drawSelection, EditorView, keymap, placeholder } from "@codemirror/view";
import { useEffect, useRef, useState } from "react";
import { yCollab, yUndoManagerKeymap } from "y-codemirror.next";
import * as Y from "yjs";
import { api, onChange, type PageEditorSession } from "../../lib/api";
import { misleadingLinkWarning, openExternalUrl } from "../../lib/contact";
import { useMembers } from "../../lib/members";
import { reportError, showToast } from "../../lib/toast";
import { linkedMessageView, useWorkspace } from "../../lib/workspace";
import { ConfirmDialog } from "../../ui";
import { ProfileCardAt } from "../members/ProfileCardAt";
import { formattingKeymap } from "./editor/commands";
import { livePreview, setKnownMembers, setPageTitles } from "./editor/livePreview";
import type { CardAnchor } from "./editor/openLinks";
import { type MenuKey, type Suggestion, suggestions } from "./editor/suggestions";
import { pageTheme } from "./editor/theme";
import { PAGE_TEXT } from "./pageText";
import { SuggestionMenu } from "./SuggestionMenu";

type OpenMemberCard = { memberId: string; anchor: CardAnchor; openedBy: "pointer" | "keyboard" };

/** Local keystrokes are batched before being sealed and sent. */
const SEND_DELAY_MS = 120;
/** A long-open editor gets a fresh client id this often, which limits how long anyone has to find a nonce that lands on it. */
const SESSION_RENEW_MS = 24 * 60 * 60 * 1000;
/** A due renewal waits while you're typing, checking again this often. */
const RENEW_RETRY_MS = 60 * 1000;
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
  return <PageEditor key={resets} pageId={pageId} restart={() => setResets((count) => count + 1)} />;
}

/** `restart` must start a fresh editor, with a new session, from the saved page. */
function PageEditor({ pageId, restart }: { pageId: string; restart: () => void }) {
  const host = useRef<HTMLDivElement>(null);
  const [view, setView] = useState<EditorView | null>(null);
  const { pages, navigate } = useWorkspace();
  const navigateRef = useRef(navigate);
  navigateRef.current = navigate;
  const restartRef = useRef(restart);
  restartRef.current = restart;
  const [suggestion, setSuggestion] = useState<Suggestion | null>(null);
  const members = useMembers();
  const [memberCard, setMemberCard] = useState<OpenMemberCard | null>(null);
  const [misleadingLink, setMisleadingLink] = useState<{ text: string; url: string } | null>(null);
  const menuKeys = useRef<(key: MenuKey, view: EditorView) => boolean>(() => false);

  useEffect(() => {
    const doc = new Y.Doc();
    const text = doc.getText(PAGE_TEXT);
    let session: PageEditorSession | undefined;
    let pending: Uint8Array[] = [];
    let timer: ReturnType<typeof setTimeout> | undefined;
    // Later updates build on earlier ones, so nobody else could apply them
    // without those: updates go out one at a time, and once one fails this
    // doc sends nothing more and a fresh editor starts from the saved page.
    let isSending = false;
    let hasStopped = false;
    const startOver = () => {
      hasStopped = true;
      restartRef.current();
    };
    const flush = () => {
      timer = undefined;
      if (pending.length === 0 || isSending || hasStopped || !session) return;
      // Yjs picks a new client id when a remote update uses this doc's own;
      // edits under it would be refused, so start over with a new session.
      if (doc.clientID !== session.client_id) {
        reportError(
          new Error(
            "Your last change clashed with another edit and wasn't saved, so the page was reloaded. Please make it again.",
          ),
        );
        startOver();
        return;
      }
      const update = Y.mergeUpdates(pending);
      pending = [];
      isSending = true;
      api.applyDocUpdate(pageId, session.nonce, update).then(
        () => {
          isSending = false;
          flush();
        },
        (error: unknown) => {
          reportError(error);
          startOver();
        },
      );
    };
    let created: EditorView | undefined;
    // Restarting drops the cursor and undo history, so it waits until you're
    // not in the editor and nothing is being saved.
    let renewal = setTimeout(function renew() {
      if (pending.length > 0 || isSending || created?.hasFocus) {
        renewal = setTimeout(renew, RENEW_RETRY_MS);
        return;
      }
      startOver();
    }, SESSION_RENEW_MS);
    doc.on("update", (update: Uint8Array, origin: unknown) => {
      if (origin === REMOTE) return;
      pending.push(update);
      timer ??= setTimeout(flush, SEND_DELAY_MS);
    });

    // Subscribe before loading so nothing that arrives in between is lost.
    const unsubscribe = onChange((change) => {
      if (change.kind === "doc" && change.page === pageId) Y.applyUpdate(doc, Uint8Array.from(change.update), REMOTE);
    });

    let disposed = false;
    Promise.all([api.pageEditorSession(pageId), api.pageState(pageId)]).then(([started, state]) => {
      if (disposed || !host.current) return;
      session = started;
      doc.clientID = started.client_id;
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
            livePreview({
              onOpenPage: (id) => navigateRef.current({ kind: "page", id }),
              onOpenMessage: (id) =>
                api.linkedMessage(id).then((linked) => {
                  if (!linked) return showToast("That message was deleted.", "info");
                  navigateRef.current(linkedMessageView(linked));
                }, reportError),
              onOpenMember: (memberId, anchor, openedBy) => setMemberCard({ memberId, anchor, openedBy }),
              onMisleadingLink: (linkText, url) => setMisleadingLink({ text: linkText, url }),
            }),
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
      setView(created);
    }, reportError);

    return () => {
      disposed = true;
      unsubscribe();
      if (timer) clearTimeout(timer);
      clearTimeout(renewal);
      flush();
      created?.destroy();
      doc.destroy();
    };
  }, [pageId]);

  useEffect(() => {
    view?.dispatch({ effects: setPageTitles.of(new Map(pages.map((page) => [page.id, page.title]))) });
  }, [view, pages]);

  useEffect(() => {
    if (members) view?.dispatch({ effects: setKnownMembers.of(new Set(members.map((member) => member.id))) });
  }, [view, members]);

  return (
    <div className="relative">
      <div ref={host} className="selectable min-h-[50vh]" />
      <SuggestionMenu view={view} suggestion={suggestion} pageId={pageId} keys={menuKeys} />
      <ConfirmDialog
        open={misleadingLink !== null}
        onOpenChange={(open) => !open && setMisleadingLink(null)}
        title="Open this link?"
        description={misleadingLink ? misleadingLinkWarning(misleadingLink.text, misleadingLink.url) : ""}
        confirmLabel="Open link"
        onConfirm={() => (misleadingLink ? openExternalUrl(misleadingLink.url) : Promise.resolve())}
      />
      {memberCard && (
        <ProfileCardAt
          {...memberCard}
          onDismiss={() => {
            setMemberCard(null);
            view?.focus();
          }}
          onLeave={() => setMemberCard(null)}
        />
      )}
    </div>
  );
}
