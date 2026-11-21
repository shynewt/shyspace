import { WidgetType } from "@codemirror/view";
import { CALLOUTS, type CalloutKind } from "../../../lib/callouts";
import { lucideIcon } from "./icons";

/** A callout's label and icon, in place of its `[!KIND]` marker. */
export class CalloutLabelWidget extends WidgetType {
  constructor(readonly kind: CalloutKind) {
    super();
  }

  eq(other: CalloutLabelWidget) {
    return other.kind === this.kind;
  }

  toDOM() {
    const { label, iconPaths } = CALLOUTS[this.kind];
    const title = document.createElement("span");
    title.className = "cm-md-callout-title";
    title.append(lucideIcon(iconPaths), label);
    return title;
  }
}
