/**
 * Files uploading into a page: each shows as an "Uploading <name>…" line
 * where its link will go, which moves with the text around it. The link is
 * written only once the upload has finished, so the page never names a file
 * that isn't there.
 */
import { type EditorState, StateEffect, StateField } from "@codemirror/state";
import { Decoration, type DecorationSet, EditorView, WidgetType } from "@codemirror/view";
import { fileIcon } from "./icons";

type Placeholder = { id: number; name: string; at: number };

const addPlaceholder = StateEffect.define<Placeholder>({
  map: (placeholder, changes) => ({ ...placeholder, at: changes.mapPos(placeholder.at) }),
});
const removePlaceholder = StateEffect.define<number>();
/** How much of the uploading file has gone, from 0 to 1. */
const setProgress = StateEffect.define<{ id: number; sent: number }>();

const placeholders = StateField.define<DecorationSet>({
  create: () => Decoration.none,
  update: (current, transaction) => {
    let next = current.map(transaction.changes);
    for (const effect of transaction.effects) {
      if (effect.is(addPlaceholder)) {
        const widget = new UploadWidget(effect.value.id, effect.value.name, 0);
        next = next.update({ add: [Decoration.widget({ widget, side: 1 }).range(effect.value.at)] });
      } else if (effect.is(removePlaceholder)) {
        next = next.update({ filter: (_from, _to, decoration) => widgetOf(decoration)?.id !== effect.value });
      } else if (effect.is(setProgress)) {
        next = withProgress(next, effect.value.id, effect.value.sent);
      }
    }
    return next;
  },
  provide: (field) => EditorView.decorations.from(field),
});

export const pageUploads = [placeholders];

let nextId = 1;

/** Shows a file starting to upload at `at`; returns the id to finish it with. */
export function startUpload(view: EditorView, name: string, at: number): number {
  const id = nextId++;
  view.dispatch({ effects: addPlaceholder.of({ id, name, at }) });
  return id;
}

export function showUploadProgress(view: EditorView, id: number, sent: number): void {
  view.dispatch({ effects: setProgress.of({ id, sent }) });
}

/**
 * Ends an upload: writes `markdown` where its line stands now, on a line of
 * its own, or with nothing to write (it failed) just takes the line away.
 */
export function finishUpload(view: EditorView, id: number, markdown: string | null): void {
  const at = placeholderPosition(view.state, id);
  if (at === null) return;
  if (markdown === null) {
    view.dispatch({ effects: removePlaceholder.of(id) });
    return;
  }
  const line = view.state.doc.lineAt(at);
  const before = at > line.from ? "\n" : "";
  const after = at < line.to ? "\n" : "";
  view.dispatch({
    changes: { from: at, insert: `${before}${markdown}${after}` },
    effects: removePlaceholder.of(id),
    userEvent: "input.paste",
  });
}

function placeholderPosition(state: EditorState, id: number): number | null {
  let found: number | null = null;
  state.field(placeholders).between(0, state.doc.length, (from, _to, decoration) => {
    if (widgetOf(decoration)?.id === id) found = from;
  });
  return found;
}

function withProgress(set: DecorationSet, id: number, sent: number): DecorationSet {
  let at: number | null = null;
  let name = "";
  set.between(0, Number.MAX_SAFE_INTEGER, (from, _to, decoration) => {
    const widget = widgetOf(decoration);
    if (widget?.id === id) {
      at = from;
      name = widget.name;
    }
  });
  if (at === null) return set;
  const widget = new UploadWidget(id, name, sent);
  return set.update({
    filter: (_from, _to, decoration) => widgetOf(decoration)?.id !== id,
    add: [Decoration.widget({ widget, side: 1 }).range(at)],
  });
}

function widgetOf(decoration: Decoration): UploadWidget | null {
  const widget = decoration.spec.widget;
  return widget instanceof UploadWidget ? widget : null;
}

class UploadWidget extends WidgetType {
  constructor(
    readonly id: number,
    readonly name: string,
    readonly sent: number,
  ) {
    super();
  }

  eq(other: UploadWidget) {
    return other.id === this.id && other.sent === this.sent;
  }

  toDOM() {
    const line = document.createElement("span");
    line.className = "cm-upload";
    const label = document.createElement("span");
    label.textContent = `Uploading ${this.name}…`;
    const bar = document.createElement("span");
    bar.className = "cm-upload-bar";
    bar.setAttribute("role", "progressbar");
    bar.setAttribute("aria-label", `Uploading ${this.name}`);
    bar.setAttribute("aria-valuemin", "0");
    bar.setAttribute("aria-valuemax", "100");
    bar.setAttribute("aria-valuenow", String(Math.round(this.sent * 100)));
    const filled = document.createElement("span");
    filled.style.transform = `scaleX(${this.sent})`;
    bar.append(filled);
    line.append(fileIcon(), label, bar);
    return line;
  }

  ignoreEvent() {
    return true;
  }
}
