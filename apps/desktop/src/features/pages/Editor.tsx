import { defaultKeymap, indentWithTab } from "@codemirror/commands";
import { markdown, markdownLanguage } from "@codemirror/lang-markdown";
import { languages } from "@codemirror/language-data";
import { EditorState, Prec } from "@codemirror/state";
import { drawSelection, EditorView, keymap, placeholder } from "@codemirror/view";
import { useCallback, useEffect, useId, useMemo, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { yCollab, yUndoManagerKeymap } from "y-codemirror.next";
import * as Y from "yjs";
import { api, onChange, onFilesDropped, type PageEditorSession, type PagePresence } from "../../lib/api";
import { saveBeforeSwitch } from "../../lib/beforeSwitch";
import { clipboardImageFile, isEmptyPaste } from "../../lib/clipboardImage";
import { misleadingLinkWarning, openExternalUrl } from "../../lib/contact";
import { addDropTarget, dropTargetAt, markDropTargetUsed, removeDropTarget } from "../../lib/dropTargets";
import { useMembers } from "../../lib/members";
import { displayName, distinctName } from "../../lib/people";
import { usePreferences } from "../../lib/preferences";
import { reportError, showToast } from "../../lib/toast";
import { useLive } from "../../lib/useLive";
import { linkedMessageView, useWorkspace } from "../../lib/workspace";
import { ConfirmDialog, memberColor, memberTint } from "../../ui";
import { ProfileCardAt } from "../members/ProfileCardAt";
import { useWorkspaceLinks } from "../shell/useWorkspaceLinks";
import { formattingKeymap } from "./editor/commands";
import {
  type ImageHost,
  type ImageProps,
  type ImageRange,
  imageAt,
  isImageSelected,
  MAX_IMAGE_WIDTH,
  selectedImage,
  setImageWidth,
} from "./editor/images";
import { livePreview, setMemberNames, setPageFileNames, setPageTitles } from "./editor/livePreview";
import type { CardAnchor } from "./editor/openLinks";
import {
  type RemoteCursor,
  relativeSelection,
  remoteCursors,
  remoteCursorTheme,
  resolveCursor,
  setRemoteCursors,
} from "./editor/remoteCursors";
import { type MenuKey, type Suggestion, suggestions } from "./editor/suggestions";
import { pageTheme } from "./editor/theme";
import { pageUploads } from "./editor/uploads";
import { PageFileViewer } from "./PageFileViewer";
import { PageImage } from "./PageImage";
import { PAGE_TEXT } from "./pageText";
import { stagePastedForPage, uploadIntoPage } from "./pageUploads";

/** How far Alt+arrow resizes an image, in percent of the column. */
const IMAGE_WIDTH_STEP = 10;
const portalKeys = new WeakMap<HTMLElement, string>();
let nextPortalKey = 0;

function portalKey(element: HTMLElement): string {
  let key = portalKeys.get(element);
  if (key === undefined) {
    key = `image-${nextPortalKey++}`;
    portalKeys.set(element, key);
  }
  return key;
}

import { mentionTarget, showMentionTarget } from "./editor/mentionTarget";
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
/** A request to go to a member's cursor; `at` makes asking twice go twice. */
export type CursorJump = { member: string; at: number };

/** Your selection goes to the others on the page at most this often. */
const CURSOR_SEND_GAP_MS = 100;

export function Editor({
  pageId,
  presence,
  jumpTo,
  mentionAt,
  isReadOnly = false,
}: {
  pageId: string;
  /** Who else is on the page, and their cursors. */
  presence: PagePresence | undefined;
  jumpTo: CursorJump | null;
  /** A spot to put the caret at and show, as an encoded Yjs relative position; a new array goes there again. */
  mentionAt?: number[];
  /** Shown, not editable: on a device that only reads until it is added. */
  isReadOnly?: boolean;
}) {
  // A Yjs doc can't forget what it merged, so a rebuilt page gets a fresh one.
  const [resets, setResets] = useState(0);
  useEffect(
    () =>
      onChange((change) => {
        if (change.kind === "page_reset" && change.page === pageId) setResets((count) => count + 1);
      }),
    [pageId],
  );
  return (
    <PageEditor
      key={resets}
      pageId={pageId}
      presence={presence}
      jumpTo={jumpTo}
      mentionAt={mentionAt}
      isReadOnly={isReadOnly}
      restart={() => setResets((count) => count + 1)}
    />
  );
}

/** `restart` must start a fresh editor, with a new session, from the saved page. */
function PageEditor({
  pageId,
  presence,
  jumpTo,
  mentionAt,
  isReadOnly,
  restart,
}: {
  pageId: string;
  presence: PagePresence | undefined;
  jumpTo: CursorJump | null;
  mentionAt: number[] | undefined;
  isReadOnly: boolean;
  restart: () => void;
}) {
  const [{ showCursorsInPages, shareCursorInPages }] = usePreferences();
  const shareRef = useRef(shareCursorInPages);
  shareRef.current = shareCursorInPages;
  const currentWorkspaceLinks = useWorkspaceLinks();
  const workspaceLinksRef = useRef(currentWorkspaceLinks);
  workspaceLinksRef.current = currentWorkspaceLinks;
  const textRef = useRef<Y.Text | null>(null);
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
  const dropTarget = useId();
  const titleRef = useRef("");
  titleRef.current = pages.find((page) => page.id === pageId)?.title ?? "";
  const [images, setImages] = useState<ReadonlyMap<HTMLElement, ImageProps>>(new Map());
  const imagesRef = useRef(images);
  imagesRef.current = images;
  const imageHost = useRef<ImageHost>({
    show: (element, props) => setImages((current) => new Map(current).set(element, props)),
    hide: (element) =>
      setImages((current) => {
        const next = new Map(current);
        next.delete(element);
        return next;
      }),
  });
  // Bumped when the selection moves while images are shown, to redraw which is selected.
  const [, setSelectionMoves] = useState(0);
  const [captionEditing, setCaptionEditing] = useState<HTMLElement | null>(null);
  const loadFiles = useCallback(() => api.pageFiles(pageId), [pageId]);
  const files = useLive(loadFiles, (change) => change.kind === "page_files" && change.page === pageId);
  const filesById = useMemo(() => new Map((files ?? []).map((file) => [file.id, file])), [files]);
  const [viewingFile, setViewingFile] = useState<string | null>(null);

  useEffect(() => {
    const doc = new Y.Doc();
    const text = doc.getText(PAGE_TEXT);
    textRef.current = text;
    let lastCursorSent = 0;
    let cursorTimer: ReturnType<typeof setTimeout> | undefined;
    const sendCursor = (editor: EditorView) => {
      if (!shareRef.current) return;
      clearTimeout(cursorTimer);
      const wait = lastCursorSent + CURSOR_SEND_GAP_MS - Date.now();
      if (wait > 0) {
        cursorTimer = setTimeout(() => sendCursor(editor), wait);
        return;
      }
      lastCursorSent = Date.now();
      const selection = editor.state.selection.main;
      const shared = editor.hasFocus ? relativeSelection(text, selection.anchor, selection.head) : null;
      api.moveCursor(pageId, shared).catch(reportError);
    };
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
    // Waiting for everything typed so far to be sent, before switching workspace.
    let whenSent: (() => void)[] = [];
    const settle = () => {
      if (isSending || (pending.length > 0 && !hasStopped && session)) return;
      for (const resolve of whenSent) resolve();
      whenSent = [];
    };
    const flush = () => {
      timer = undefined;
      if (pending.length === 0 || isSending || hasStopped || !session) return settle();
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
          isSending = false;
          reportError(error);
          startOver();
          settle();
        },
      );
    };
    const stopSavingBeforeSwitch = saveBeforeSwitch(() => {
      if (timer) clearTimeout(timer);
      const sent = new Promise<void>((resolve) => whenSent.push(resolve));
      flush();
      return sent;
    });
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
            EditorState.readOnly.of(isReadOnly),
            EditorView.editable.of(!isReadOnly),
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
              onOpenFile: setViewingFile,
              images: imageHost.current,
              links: () => workspaceLinksRef.current,
            }),
            Prec.highest(
              keymap.of([
                {
                  key: "Alt-ArrowLeft",
                  run: (editor) => resizeSelected(editor, -IMAGE_WIDTH_STEP),
                  stopPropagation: true,
                },
                {
                  key: "Alt-ArrowRight",
                  run: (editor) => resizeSelected(editor, IMAGE_WIDTH_STEP),
                  stopPropagation: true,
                },
                { key: "Alt-0", run: (editor) => resetSelected(editor), stopPropagation: true },
                {
                  key: "Enter",
                  run: (editor) => {
                    const image = selectedImage(editor.state);
                    const element = image && elementOf(editor, imagesRef.current, image);
                    if (!element) return false;
                    setCaptionEditing(element);
                    return true;
                  },
                },
              ]),
            ),
            EditorView.updateListener.of((update) => {
              if (update.selectionSet && imagesRef.current.size > 0) setSelectionMoves((count) => count + 1);
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
            pageUploads,
            remoteCursors(text),
            mentionTarget,
            remoteCursorTheme,
            EditorView.updateListener.of((update) => {
              if (update.selectionSet || update.focusChanged) sendCursor(update.view);
            }),
            EditorView.domEventHandlers({
              focus: () => markDropTargetUsed(dropTarget),
              paste: (event, editor) => {
                const clipboard = event.clipboardData;
                if (!clipboard) return false;
                const isEmpty = isEmptyPaste(clipboard);
                const files = [...clipboard.files];
                if (files.length === 0 && !isEmpty) return false;
                event.preventDefault();
                (isEmpty ? clipboardImageFile().then((file) => [file]) : Promise.resolve(files))
                  .then((pasted) => stagePastedForPage(pasted, pageId, titleRef.current))
                  .then((staged) => uploadIntoPage(editor, pageId, staged))
                  .catch(reportError);
                return true;
              },
            }),
          ],
        }),
      });
      setView(created);
    }, reportError);

    return () => {
      disposed = true;
      stopSavingBeforeSwitch();
      unsubscribe();
      if (timer) clearTimeout(timer);
      clearTimeout(renewal);
      flush();
      created?.destroy();
      doc.destroy();
    };
  }, [pageId, dropTarget, isReadOnly]);

  useEffect(() => {
    if (!view) return;
    addDropTarget(dropTarget, () => view.dom);
    const stopListening = onFilesDropped(({ files, position }) => {
      if (dropTargetAt(position) === dropTarget) uploadIntoPage(view, pageId, files).catch(reportError);
    });
    return () => {
      stopListening();
      removeDropTarget(dropTarget);
    };
  }, [view, pageId, dropTarget]);

  useEffect(() => {
    view?.dispatch({ effects: setPageTitles.of(new Map(pages.map((page) => [page.id, page.title]))) });
  }, [view, pages]);

  // Joining the open page again only switches sharing, so a change of the
  // setting never leaves the page in between.
  useEffect(() => {
    api.joinPage(pageId, shareCursorInPages).catch(reportError);
  }, [pageId, shareCursorInPages]);
  useEffect(
    () => () => {
      api.leavePage(pageId).catch(reportError);
    },
    [pageId],
  );

  useEffect(() => {
    if (!view) return;
    const now = Date.now();
    const cursors: RemoteCursor[] =
      showCursorsInPages && presence
        ? presence.cursors.flatMap(({ member, selection, idle_ms }) =>
            selection
              ? [
                  {
                    member,
                    name: displayName(members?.find((candidate) => candidate.id === member)?.name ?? "", member),
                    color: memberColor(member),
                    tint: memberTint(member),
                    anchor: Uint8Array.from(selection.anchor),
                    head: Uint8Array.from(selection.head),
                    movedAt: now - idle_ms,
                  },
                ]
              : [],
          )
        : [];
    view.dispatch({ effects: setRemoteCursors.of(cursors) });
  }, [view, presence, showCursorsInPages, members]);

  const handledJump = useRef<CursorJump | null>(null);
  useEffect(() => {
    if (!jumpTo || handledJump.current === jumpTo) return;
    const cursor = presence?.cursors.find((candidate) => candidate.member === jumpTo.member);
    const text = textRef.current;
    if (!view || !text || !cursor?.selection) return;
    handledJump.current = jumpTo;
    const resolved = resolveCursor(text, {
      anchor: Uint8Array.from(cursor.selection.anchor),
      head: Uint8Array.from(cursor.selection.head),
    });
    if (resolved) view.dispatch({ effects: EditorView.scrollIntoView(resolved.head, { y: "center" }) });
  }, [view, jumpTo, presence]);

  const handledMention = useRef<number[] | null>(null);
  useEffect(() => {
    const text = textRef.current;
    if (!mentionAt || handledMention.current === mentionAt || !view || !text?.doc) return;
    handledMention.current = mentionAt;
    const position = Y.createAbsolutePositionFromRelativePosition(
      Y.decodeRelativePosition(Uint8Array.from(mentionAt)),
      text.doc,
    );
    // Text that hasn't arrived yet has no place; the page's start is the nearest one known.
    const at = position?.type === text ? Math.min(position.index, view.state.doc.length) : 0;
    view.dispatch({
      selection: { anchor: at },
      effects: [EditorView.scrollIntoView(at, { y: "center" }), showMentionTarget.of(at)],
    });
    view.focus();
  }, [view, mentionAt]);

  useEffect(() => {
    view?.dispatch({ effects: setPageFileNames.of(new Map([...filesById].map(([id, file]) => [id, file.name]))) });
  }, [view, filesById]);

  useEffect(() => {
    if (!members) return;
    const names = new Map(members.map((member) => [member.id, distinctName(member, members)]));
    view?.dispatch({ effects: setMemberNames.of(names) });
  }, [view, members]);

  const viewedFile = viewingFile ? filesById.get(viewingFile) : undefined;
  return (
    <div className="relative">
      <div ref={host} className="selectable min-h-[50vh]" />
      {view &&
        [...images].map(([element, props]) =>
          createPortal(
            <PageImage
              view={view}
              element={element}
              pageId={pageId}
              {...props}
              file={filesById.get(props.fileId)}
              isSelected={isDrawnImageSelected(view, element)}
              isEditingCaption={captionEditing === element}
              onCaptionEditEnd={() => setCaptionEditing(null)}
            />,
            element,
            portalKey(element),
          ),
        )}
      {viewedFile && (
        <PageFileViewer
          pageId={pageId}
          file={viewedFile}
          onClose={() => {
            setViewingFile(null);
            view?.focus();
          }}
        />
      )}
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

/** Alt+arrows: the selected image, a step narrower or wider. */
function resizeSelected(view: EditorView, step: number): boolean {
  const image = selectedImage(view.state);
  if (!image) return false;
  setImageWidth(view, image, (image.width ?? MAX_IMAGE_WIDTH) + step);
  return true;
}

/** Alt+0: the selected image back to its own size, at most the column's. */
function resetSelected(view: EditorView): boolean {
  const image = selectedImage(view.state);
  if (!image) return false;
  setImageWidth(view, image, null);
  return true;
}

function elementOf(
  view: EditorView,
  images: ReadonlyMap<HTMLElement, ImageProps>,
  image: ImageRange,
): HTMLElement | null {
  for (const element of images.keys()) {
    if (isAt(view, element, image)) return element;
  }
  return null;
}

/** Whether the selection takes in the image this widget's element draws. */
function isDrawnImageSelected(view: EditorView, element: HTMLElement): boolean {
  if (!element.isConnected) return false;
  const image = imageAt(view.state, view.posAtDOM(element));
  return image !== null && isImageSelected(view.state, image);
}

/** Whether a widget's element draws `image`: its spot in the document falls inside the image's markdown. */
function isAt(view: EditorView, element: HTMLElement, image: ImageRange): boolean {
  if (!element.isConnected) return false;
  const position = view.posAtDOM(element);
  return position >= image.from && position <= image.to;
}
