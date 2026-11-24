/**
 * Other members' cursors and selections in a page, drawn in their avatar's
 * hue. A cursor's name shows for a moment after it moves and on hover, so it
 * never sits over the text for long; a cursor that hasn't moved for a minute
 * fades. Positions arrive as Yjs relative positions and are resolved against
 * the page's text here, so they stay put as the text around them changes.
 */
import { type Range, StateEffect, StateField } from "@codemirror/state";
import { Decoration, type DecorationSet, EditorView, ViewPlugin, type ViewUpdate, WidgetType } from "@codemirror/view";
import * as Y from "yjs";

/** How long a cursor's name shows after it moves. */
export const NAME_SHOWN_MS = 2000;
/** A cursor that hasn't moved for this long fades. */
export const IDLE_AFTER_MS = 60 * 1000;

export type RemoteCursor = {
  member: string;
  name: string;
  /** The member's colour, and its faint form for their selection. */
  color: string;
  tint: string;
  anchor: Uint8Array;
  head: Uint8Array;
  /** When it last moved, by this device's clock. */
  movedAt: number;
};

export const setRemoteCursors = StateEffect.define<readonly RemoteCursor[]>();
/** Time passed: names hide and cursors fade, though nothing moved. */
const timePassed = StateEffect.define<null>();

const cursorsField = StateField.define<readonly RemoteCursor[]>({
  create: () => [],
  update: (cursors, transaction) => {
    let next = cursors;
    for (const effect of transaction.effects) if (effect.is(setRemoteCursors)) next = effect.value;
    return next;
  },
});

/** Where a cursor is in `text` now, or null if its text is gone or hasn't arrived. */
export function resolveCursor(text: Y.Text, cursor: Pick<RemoteCursor, "anchor" | "head">) {
  const doc = text.doc;
  if (!doc) return null;
  const at = (encoded: Uint8Array) => {
    const position = Y.createAbsolutePositionFromRelativePosition(Y.decodeRelativePosition(encoded), doc);
    return position && position.type === text ? position.index : null;
  };
  const anchor = at(cursor.anchor);
  const head = at(cursor.head);
  return anchor === null || head === null ? null : { anchor, head };
}

/** This member's selection as relative positions, to send to the others on the page. */
export function relativeSelection(text: Y.Text, anchor: number, head: number) {
  const encode = (index: number) =>
    Array.from(Y.encodeRelativePosition(Y.createRelativePositionFromTypeIndex(text, index)));
  return { anchor: encode(anchor), head: encode(head) };
}

export function remoteCursors(text: Y.Text) {
  const plugin = ViewPlugin.fromClass(
    class {
      decorations: DecorationSet = Decoration.none;
      private timer: ReturnType<typeof setTimeout> | undefined;

      constructor(private readonly view: EditorView) {
        this.decorations = this.build();
      }

      update(update: ViewUpdate) {
        const isTimePassing = update.transactions.some((transaction) =>
          transaction.effects.some((effect) => effect.is(timePassed)),
        );
        if (
          update.docChanged ||
          isTimePassing ||
          update.startState.field(cursorsField) !== update.state.field(cursorsField)
        ) {
          this.decorations = this.build();
        }
      }

      destroy() {
        clearTimeout(this.timer);
      }

      private build(): DecorationSet {
        clearTimeout(this.timer);
        const now = Date.now();
        const length = this.view.state.doc.length;
        const ranges: Range<Decoration>[] = [];
        let nextChange = Number.POSITIVE_INFINITY;
        for (const cursor of this.view.state.field(cursorsField)) {
          const resolved = resolveCursor(text, cursor);
          if (!resolved) continue;
          const anchor = Math.min(resolved.anchor, length);
          const head = Math.min(resolved.head, length);
          const age = now - cursor.movedAt;
          for (const threshold of [NAME_SHOWN_MS, IDLE_AFTER_MS]) {
            if (age < threshold) nextChange = Math.min(nextChange, threshold - age);
          }
          const from = Math.min(anchor, head);
          const to = Math.max(anchor, head);
          if (from < to) {
            ranges.push(
              Decoration.mark({
                class: "cm-remote-selection",
                attributes: { style: `background-color: ${cursor.tint}` },
              }).range(from, to),
            );
          }
          const caret = new CaretWidget(cursor.name, cursor.color, age < NAME_SHOWN_MS, age >= IDLE_AFTER_MS);
          ranges.push(Decoration.widget({ widget: caret, side: head > anchor ? -1 : 1 }).range(head));
        }
        if (Number.isFinite(nextChange)) {
          this.timer = setTimeout(() => this.view.dispatch({ effects: timePassed.of(null) }), nextChange + 10);
        }
        return Decoration.set(ranges, true);
      }
    },
    { decorations: (value) => value.decorations },
  );
  return [cursorsField, plugin];
}

class CaretWidget extends WidgetType {
  constructor(
    readonly name: string,
    readonly color: string,
    readonly isNameShown: boolean,
    readonly isIdle: boolean,
  ) {
    super();
  }

  eq(other: CaretWidget) {
    return (
      other.name === this.name &&
      other.color === this.color &&
      other.isNameShown === this.isNameShown &&
      other.isIdle === this.isIdle
    );
  }

  toDOM() {
    const caret = document.createElement("span");
    caret.className = "cm-remote-caret";
    if (this.isNameShown) caret.classList.add("is-fresh");
    if (this.isIdle) caret.classList.add("is-idle");
    caret.style.borderLeftColor = this.color;
    caret.setAttribute("aria-hidden", "true");
    const label = document.createElement("span");
    label.className = "cm-remote-label";
    label.textContent = this.name;
    label.style.backgroundColor = this.color;
    caret.append(label);
    return caret;
  }

  ignoreEvent() {
    return false;
  }
}

/** Carets are thin lines with a label above that shows when fresh or hovered. */
export const remoteCursorTheme = EditorView.baseTheme({
  ".cm-remote-caret": {
    position: "relative",
    borderLeft: "2px solid",
    marginLeft: "-1px",
    marginRight: "-1px",
    transition: "opacity 300ms",
  },
  // A wider, invisible target, so the caret's name shows on hover.
  ".cm-remote-caret::after": {
    content: "''",
    position: "absolute",
    top: "0",
    bottom: "0",
    left: "-5px",
    width: "10px",
    zIndex: "1",
  },
  ".cm-remote-caret.is-idle": { opacity: "0.35" },
  ".cm-remote-label": {
    position: "absolute",
    bottom: "100%",
    left: "-2px",
    padding: "0 4px",
    borderRadius: "4px 4px 4px 0",
    color: "white",
    fontSize: "0.7em",
    fontWeight: "600",
    lineHeight: "1.5",
    whiteSpace: "nowrap",
    pointerEvents: "none",
    visibility: "hidden",
  },
  ".cm-remote-caret.is-fresh .cm-remote-label, .cm-remote-caret:hover .cm-remote-label": { visibility: "visible" },
});
