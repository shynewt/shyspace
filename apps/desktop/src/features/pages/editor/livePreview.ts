/**
 * Live preview: the page stays markdown, but lines nobody is editing read as
 * formatted text. Decorations only change how the document looks; widgets
 * that act (checkboxes, page chips) make ordinary edits or navigate.
 */
import { syntaxTree } from "@codemirror/language";
import { type Range, StateEffect, StateField } from "@codemirror/state";
import {
  Decoration,
  type DecorationSet,
  EditorView,
  hoverTooltip,
  ViewPlugin,
  type ViewUpdate,
  WidgetType,
} from "@codemirror/view";
import { MOD_LABEL } from "../../../lib/shortcuts";
import { CalloutLabelWidget } from "./callouts";
import { fileIcon, messageIcon, pageIcon } from "./icons";
import { type ImageHost, type ImageProps, imageHost } from "./images";
import { externalUrl, linkAt } from "./links";
import { previewItems } from "./markdownPreview";
import {
  fileOpener,
  type MemberOpener,
  type MisleadingLinkAsker,
  memberOpener,
  messageOpener,
  misleadingLinkAsker,
  openFile,
  openLink,
  openMember,
  openMessage,
  openPage,
  pageOpener,
} from "./openLinks";
import { tablePreview } from "./tables";
import { toggleTaskAt } from "./tasks";

/** The workspace's page titles, for drawing page links by their current title. */
export const setPageTitles = StateEffect.define<ReadonlyMap<string, string>>();

const pageTitles = StateField.define<ReadonlyMap<string, string>>({
  create: () => new Map(),
  update: (titles, transaction) => {
    let next = titles;
    for (const effect of transaction.effects) if (effect.is(setPageTitles)) next = effect.value;
    return next;
  },
});

/** The page's own files by id, with their current names; links to any other file draw as unavailable. */
export const setPageFileNames = StateEffect.define<ReadonlyMap<string, string>>();

const pageFileNames = StateField.define<ReadonlyMap<string, string>>({
  create: () => new Map(),
  update: (names, transaction) => {
    let next = names;
    for (const effect of transaction.effects) if (effect.is(setPageFileNames)) next = effect.value;
    return next;
  },
});

/** The ids of the workspace's members, so only mentions of someone known open a card. */
export const setKnownMembers = StateEffect.define<ReadonlySet<string>>();

export const knownMembers = StateField.define<ReadonlySet<string>>({
  create: () => new Set(),
  update: (known, transaction) => {
    let next = known;
    for (const effect of transaction.effects) if (effect.is(setKnownMembers)) next = effect.value;
    return next;
  },
});

export function livePreview({
  onOpenPage,
  onOpenMessage,
  onOpenMember,
  onMisleadingLink,
  onOpenFile,
  images,
}: {
  onOpenPage: (pageId: string) => void;
  onOpenMessage: (messageId: string) => void;
  onOpenMember: MemberOpener;
  onMisleadingLink: MisleadingLinkAsker;
  onOpenFile: (fileId: string) => void;
  /** Draws a page's images into their widgets. */
  images: ImageHost;
}) {
  return [
    pageTitles,
    pageFileNames,
    knownMembers,
    fileOpener.of(onOpenFile),
    imageHost.of(images),
    pageOpener.of(onOpenPage),
    messageOpener.of(onOpenMessage),
    memberOpener.of(onOpenMember),
    misleadingLinkAsker.of(onMisleadingLink),
    previewPlugin,
    tablePreview,
    EditorView.atomicRanges.of((view) => view.plugin(previewPlugin)?.chips ?? Decoration.none),
    linkTooltip,
    linkClicks,
  ];
}

const previewPlugin = ViewPlugin.fromClass(
  class {
    decorations: DecorationSet = Decoration.none;
    /** Page and mention chips: the cursor steps over each as one unit. */
    chips: DecorationSet = Decoration.none;
    private revealed = new Set<number>();
    /** While a mouse button is down, lines keep their look so text doesn't jump under the pointer. */
    private isPointerDown = false;
    private readonly onPointerUp: () => void;
    /** A button released outside the window sends no mouseup here; the pointer comes back with none held. */
    private readonly onPointerMove: (event: MouseEvent) => void;

    constructor(private readonly view: EditorView) {
      this.revealed = revealedLines(view);
      this.build();
      this.onPointerUp = () => {
        if (!this.isPointerDown) return;
        this.isPointerDown = false;
        view.dispatch({});
      };
      this.onPointerMove = (event) => {
        if (event.buttons === 0) this.onPointerUp();
      };
      const page = view.dom.ownerDocument;
      page.addEventListener("mouseup", this.onPointerUp);
      page.addEventListener("pointercancel", this.onPointerUp);
      page.addEventListener("mousemove", this.onPointerMove);
      page.defaultView?.addEventListener("blur", this.onPointerUp);
    }

    pointerDown() {
      this.isPointerDown = true;
    }

    update(update: ViewUpdate) {
      const revealed = this.isPointerDown ? this.revealed : revealedLines(update.view);
      const isRevealChanged = !sameLines(revealed, this.revealed);
      this.revealed = revealed;
      if (
        update.docChanged ||
        update.viewportChanged ||
        isRevealChanged ||
        syntaxTree(update.startState) !== syntaxTree(update.state) ||
        update.startState.field(pageTitles) !== update.state.field(pageTitles) ||
        update.startState.field(pageFileNames) !== update.state.field(pageFileNames) ||
        update.startState.field(knownMembers) !== update.state.field(knownMembers)
      ) {
        this.build();
      }
    }

    destroy() {
      const page = this.view.dom.ownerDocument;
      page.removeEventListener("mouseup", this.onPointerUp);
      page.removeEventListener("pointercancel", this.onPointerUp);
      page.removeEventListener("mousemove", this.onPointerMove);
      page.defaultView?.removeEventListener("blur", this.onPointerUp);
    }

    private build() {
      const titles = this.view.state.field(pageTitles);
      const fileNames = this.view.state.field(pageFileNames);
      const known = this.view.state.field(knownMembers);
      const decorations: Range<Decoration>[] = [];
      const chips: Range<Decoration>[] = [];
      for (const item of previewItems(
        this.view.state.doc,
        syntaxTree(this.view.state),
        this.view.visibleRanges,
        this.revealed,
      )) {
        switch (item.kind) {
          case "hide":
            decorations.push(Decoration.replace({}).range(item.from, item.to));
            break;
          case "line":
            decorations.push(Decoration.line({ class: item.className }).range(item.from));
            break;
          case "style":
            decorations.push(Decoration.mark({ class: item.className }).range(item.from, item.to));
            break;
          case "bullet":
            decorations.push(Decoration.replace({ widget: new BulletWidget(item.depth) }).range(item.from, item.to));
            break;
          case "checkbox":
            decorations.push(
              Decoration.replace({ widget: new CheckboxWidget(item.isChecked, item.label) }).range(item.from, item.to),
            );
            break;
          case "divider":
            decorations.push(Decoration.replace({ widget: new DividerWidget() }).range(item.from, item.to));
            break;
          case "callout":
            decorations.push(
              Decoration.replace({ widget: new CalloutLabelWidget(item.callout) }).range(item.from, item.to),
            );
            break;
          case "page": {
            const chip = Decoration.replace({ widget: new PageChipWidget(item.pageId, titles.get(item.pageId)) });
            decorations.push(chip.range(item.from, item.to));
            chips.push(chip.range(item.from, item.to));
            break;
          }
          case "message": {
            const chip = Decoration.replace({ widget: new MessageChipWidget(item.messageId, item.label) });
            decorations.push(chip.range(item.from, item.to));
            chips.push(chip.range(item.from, item.to));
            break;
          }
          case "file": {
            const chip = Decoration.replace({ widget: new FileChipWidget(item.fileId, fileNames.get(item.fileId)) });
            decorations.push(chip.range(item.from, item.to));
            chips.push(chip.range(item.from, item.to));
            break;
          }
          case "image": {
            const { fileId, width, caption } = item;
            const chip = Decoration.replace({ widget: new ImageWidget({ fileId, width, caption }) });
            decorations.push(chip.range(item.from, item.to));
            chips.push(chip.range(item.from, item.to));
            break;
          }
          case "mention": {
            const memberId = item.memberId !== null && known.has(item.memberId) ? item.memberId : null;
            const chip = Decoration.replace({ widget: new MentionWidget(item.label, memberId) });
            decorations.push(chip.range(item.from, item.to));
            chips.push(chip.range(item.from, item.to));
            break;
          }
        }
      }
      this.decorations = Decoration.set(decorations, true);
      this.chips = Decoration.set(chips, true);
    }
  },
  {
    decorations: (plugin) => plugin.decorations,
    eventHandlers: {
      mousedown() {
        this.pointerDown();
      },
    },
  },
);

/** Lines touched by a selection show their markdown; none do while the editor isn't focused. */
function revealedLines(view: EditorView): Set<number> {
  const lines = new Set<number>();
  if (!view.hasFocus) return lines;
  const { doc } = view.state;
  for (const range of view.state.selection.ranges) {
    const last = doc.lineAt(range.to).number;
    for (let line = doc.lineAt(range.from).number; line <= last; line += 1) lines.add(line);
  }
  return lines;
}

function sameLines(left: ReadonlySet<number>, right: ReadonlySet<number>): boolean {
  return left.size === right.size && [...left].every((line) => right.has(line));
}

class BulletWidget extends WidgetType {
  constructor(readonly depth: number) {
    super();
  }

  eq(other: BulletWidget) {
    return other.depth === this.depth;
  }

  toDOM() {
    const bullet = document.createElement("span");
    bullet.className = `cm-md-bullet cm-md-bullet-${this.depth % 3}`;
    bullet.setAttribute("aria-hidden", "true");
    return bullet;
  }
}

class CheckboxWidget extends WidgetType {
  constructor(
    readonly isChecked: boolean,
    readonly label: string,
  ) {
    super();
  }

  eq(other: CheckboxWidget) {
    return other.isChecked === this.isChecked && other.label === this.label;
  }

  toDOM(view: EditorView) {
    const box = document.createElement("input");
    box.type = "checkbox";
    box.className = "cm-md-checkbox";
    box.checked = this.isChecked;
    box.tabIndex = -1;
    box.setAttribute("aria-label", this.label || "Checklist item");
    // Keep the cursor where it is: moving it here would reveal the line's markdown.
    box.addEventListener("mousedown", (event) => event.preventDefault());
    box.addEventListener("click", (event) => {
      // The page decides the box's state; the click only asks to change it.
      event.preventDefault();
      toggleTaskAt(view, view.posAtDOM(box));
    });
    return box;
  }
}

class DividerWidget extends WidgetType {
  eq() {
    return true;
  }

  toDOM() {
    const divider = document.createElement("span");
    divider.className = "cm-md-divider";
    divider.setAttribute("aria-hidden", "true");
    return divider;
  }
}

class PageChipWidget extends WidgetType {
  /** `title` is undefined when the page isn't in the workspace (deleted, or not synced yet). */
  constructor(
    readonly pageId: string,
    readonly title: string | undefined,
  ) {
    super();
  }

  eq(other: PageChipWidget) {
    return other.pageId === this.pageId && other.title === this.title;
  }

  toDOM(view: EditorView) {
    const chip = document.createElement("span");
    chip.className = "cm-md-page";
    if (this.title === undefined) {
      chip.classList.add("cm-md-page-missing");
      chip.textContent = "Missing page";
      chip.title = "This page was deleted, or hasn't arrived on this device yet";
      return chip;
    }
    chip.setAttribute("role", "link");
    chip.append(pageIcon(), this.title || "Untitled");
    chip.addEventListener("mousedown", (event) => event.preventDefault());
    chip.addEventListener("click", (event) => {
      event.preventDefault();
      openPage(view, this.pageId);
    });
    return chip;
  }
}

class FileChipWidget extends WidgetType {
  /** `name` is undefined when the file isn't one of this page's (deleted, elsewhere, or not arrived yet). */
  constructor(
    readonly fileId: string,
    readonly name: string | undefined,
  ) {
    super();
  }

  eq(other: FileChipWidget) {
    return other.fileId === this.fileId && other.name === this.name;
  }

  toDOM(view: EditorView) {
    const chip = document.createElement("span");
    chip.className = "cm-md-page";
    if (this.name === undefined) {
      chip.classList.add("cm-md-page-missing");
      chip.textContent = "File unavailable";
      chip.title = "This file was deleted, belongs somewhere else, or hasn't arrived on this device yet";
      return chip;
    }
    chip.setAttribute("role", "link");
    chip.append(fileIcon(), this.name);
    chip.addEventListener("mousedown", (event) => event.preventDefault());
    chip.addEventListener("click", (event) => {
      event.preventDefault();
      openFile(view, this.fileId);
    });
    return chip;
  }
}

/** A page's image: a block the screen hosting the editor draws into. */
class ImageWidget extends WidgetType {
  constructor(readonly props: ImageProps) {
    super();
  }

  eq(other: ImageWidget) {
    return (
      other.props.fileId === this.props.fileId &&
      other.props.width === this.props.width &&
      other.props.caption === this.props.caption
    );
  }

  toDOM(view: EditorView) {
    const element = document.createElement("span");
    element.className = "cm-page-image";
    this.host = view.state.facet(imageHost);
    this.host?.show(element, this.props);
    return element;
  }

  updateDOM(element: HTMLElement, view: EditorView) {
    this.host = view.state.facet(imageHost);
    this.host?.show(element, this.props);
    return true;
  }

  destroy(element: HTMLElement) {
    this.host?.hide(element);
  }

  /** The handles, caption and viewer handle their own pointer and keys. */
  ignoreEvent() {
    return true;
  }

  /** Remembered for `destroy`, which gets no view. */
  private host: ImageHost | undefined;
}

class MessageChipWidget extends WidgetType {
  constructor(
    readonly messageId: string,
    readonly label: string,
  ) {
    super();
  }

  eq(other: MessageChipWidget) {
    return other.messageId === this.messageId && other.label === this.label;
  }

  toDOM(view: EditorView) {
    const chip = document.createElement("span");
    chip.className = "cm-md-page";
    chip.setAttribute("role", "link");
    chip.append(messageIcon(), this.label);
    chip.addEventListener("mousedown", (event) => event.preventDefault());
    chip.addEventListener("click", (event) => {
      event.preventDefault();
      openMessage(view, this.messageId);
    });
    return chip;
  }
}

class MentionWidget extends WidgetType {
  /** `memberId` is null for @channel, @here and people who aren't members: those open nothing. */
  constructor(
    readonly label: string,
    readonly memberId: string | null,
  ) {
    super();
  }

  eq(other: MentionWidget) {
    return other.label === this.label && other.memberId === this.memberId;
  }

  toDOM(view: EditorView) {
    const mention = document.createElement("span");
    mention.className = "cm-md-mention";
    mention.textContent = this.label;
    const { memberId } = this;
    if (!memberId) return mention;
    mention.setAttribute("role", "button");
    mention.setAttribute("aria-label", `${this.label.replace(/^@/, "")}, open profile`);
    // The cursor stays where it was: the card opens on top of the page, which doesn't change.
    mention.addEventListener("mousedown", (event) => event.preventDefault());
    mention.addEventListener("click", (event) => {
      event.preventDefault();
      openMember(view, memberId, mention, "pointer");
    });
    return mention;
  }
}

/** Hovering a web or mail link shows where it goes. */
const linkTooltip = hoverTooltip(
  (view, position) => {
    const link = linkAt(view.state, position);
    if (!link || link.pageId || link.messageId) return null;
    return {
      pos: link.from,
      end: link.to,
      above: true,
      create: () => {
        const dom = document.createElement("div");
        dom.className = "cm-link-tooltip";
        dom.setAttribute("role", "tooltip");
        const address = document.createElement("span");
        address.className = "cm-link-tooltip-url";
        address.textContent = link.url;
        dom.append(address);
        if (externalUrl(link.url)) {
          const hint = document.createElement("span");
          hint.className = "cm-link-tooltip-hint";
          hint.textContent = `${MOD_LABEL}+click to open`;
          dom.append(hint);
        }
        return { dom };
      },
    };
  },
  { hoverTime: 350 },
);

/** Ctrl or Cmd+click opens a link; a plain click places the cursor, to edit it. */
const linkClicks = EditorView.domEventHandlers({
  mousedown(event, view) {
    if (event.button !== 0 || !(event.ctrlKey || event.metaKey)) return false;
    const position = view.posAtCoords({ x: event.clientX, y: event.clientY });
    const link = position === null ? null : linkAt(view.state, position);
    if (!link) return false;
    event.preventDefault();
    openLink(view, link);
    return true;
  },
});
