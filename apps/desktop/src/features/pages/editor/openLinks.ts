/** Opening a page's links: web and mail links in their apps, page links in shyspace, mentions as profile cards. */
import { Facet } from "@codemirror/state";
import type { EditorView } from "@codemirror/view";
import { isMisleadingLink, openExternal } from "../../../lib/contact";
import { reportError } from "../../../lib/toast";
import { externalUrl, type LinkTarget } from "./links";

/** How the editor opens another page; given by the screen that hosts it. */
export const pageOpener = Facet.define<(pageId: string) => void, ((pageId: string) => void) | undefined>({
  combine: (openers) => openers[0],
});

/** Something a floating card can be placed against: a chip, or the cursor's spot. */
export type CardAnchor = { getBoundingClientRect: () => DOMRect };

/** How the editor shows a mentioned member's card; given by the screen that hosts it. */
export type MemberOpener = (memberId: string, anchor: CardAnchor, openedBy: "pointer" | "keyboard") => void;

export const memberOpener = Facet.define<MemberOpener, MemberOpener | undefined>({
  combine: (openers) => openers[0],
});

/** How the editor asks before opening a link whose text could mislead about where it goes; given by the screen that hosts it. */
export type MisleadingLinkAsker = (text: string, url: string) => void;

export const misleadingLinkAsker = Facet.define<MisleadingLinkAsker, MisleadingLinkAsker | undefined>({
  combine: (askers) => askers[0],
});

export function openMember(view: EditorView, memberId: string, anchor: CardAnchor, openedBy: "pointer" | "keyboard") {
  const open = view.state.facet(memberOpener);
  if (!open) throw new Error("the page editor was set up without a way to show members");
  open(memberId, anchor, openedBy);
}

export function openLink(view: EditorView, link: LinkTarget): void {
  if (link.pageId) {
    openPage(view, link.pageId);
    return;
  }
  const url = externalUrl(link.url);
  if (!url) {
    reportError(new Error(`only web and email links open from a page, not "${link.url}"`));
    return;
  }
  if (isMisleadingLink(link.text, url)) {
    const ask = view.state.facet(misleadingLinkAsker);
    if (!ask) throw new Error("the page editor was set up without a way to ask about misleading links");
    ask(link.text, url);
    return;
  }
  openExternal(url);
}

export function openPage(view: EditorView, pageId: string): void {
  const open = view.state.facet(pageOpener);
  if (!open) throw new Error("the page editor was set up without a way to open pages");
  open(pageId);
}
