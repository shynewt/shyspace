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
import { externalUrl, linkAt } from "./links";
import { previewItems } from "./markdownPreview";
import { openLink, openPage, pageOpener } from "./openLinks";
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

export function livePreview({ onOpenPage }: { onOpenPage: (pageId: string) => void }) {
  return [
    pageTitles,
    pageOpener.of(onOpenPage),
    previewPlugin,
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

    constructor(private readonly view: EditorView) {
      this.revealed = revealedLines(view);
      this.build();
      this.onPointerUp = () => {
        if (!this.isPointerDown) return;
        this.isPointerDown = false;
        view.dispatch({});
      };
      view.dom.ownerDocument.addEventListener("mouseup", this.onPointerUp);
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
        update.startState.field(pageTitles) !== update.state.field(pageTitles)
      ) {
        this.build();
      }
    }

    destroy() {
      this.view.dom.ownerDocument.removeEventListener("mouseup", this.onPointerUp);
    }

    private build() {
      const titles = this.view.state.field(pageTitles);
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
          case "page": {
            const chip = Decoration.replace({ widget: new PageChipWidget(item.pageId, titles.get(item.pageId)) });
            decorations.push(chip.range(item.from, item.to));
            chips.push(chip.range(item.from, item.to));
            break;
          }
          case "mention": {
            const chip = Decoration.replace({ widget: new MentionWidget(item.label) });
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

class MentionWidget extends WidgetType {
  constructor(readonly label: string) {
    super();
  }

  eq(other: MentionWidget) {
    return other.label === this.label;
  }

  toDOM() {
    const mention = document.createElement("span");
    mention.className = "cm-md-mention";
    mention.textContent = this.label;
    return mention;
  }
}

/** Lucide's "file-text", drawn without React since widgets are plain DOM. */
function pageIcon(): SVGSVGElement {
  const namespace = "http://www.w3.org/2000/svg";
  const icon = document.createElementNS(namespace, "svg");
  for (const [name, value] of Object.entries({
    viewBox: "0 0 24 24",
    width: "0.95em",
    height: "0.95em",
    fill: "none",
    stroke: "currentColor",
    "stroke-width": "2",
    "stroke-linecap": "round",
    "stroke-linejoin": "round",
    "aria-hidden": "true",
  })) {
    icon.setAttribute(name, value);
  }
  for (const path of [
    "M15 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V7Z",
    "M14 2v4a2 2 0 0 0 2 2h4",
    "M10 9H8",
    "M16 13H8",
    "M16 17H8",
  ]) {
    const line = document.createElementNS(namespace, "path");
    line.setAttribute("d", path);
    icon.append(line);
  }
  return icon;
}

/** Hovering a web or mail link shows where it goes. */
const linkTooltip = hoverTooltip(
  (view, position) => {
    const link = linkAt(view.state, position);
    if (!link || link.pageId) return null;
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
