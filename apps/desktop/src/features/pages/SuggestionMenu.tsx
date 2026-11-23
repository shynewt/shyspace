import type { EditorView } from "@codemirror/view";
import {
  Code,
  FileText,
  Heading1,
  Heading2,
  Heading3,
  List,
  ListChecks,
  ListOrdered,
  MessageSquareWarning,
  Minus,
  Quote,
  Table,
  Type,
} from "lucide-react";
import { motion } from "motion/react";
import { type ReactNode, type RefObject, useCallback, useId, useLayoutEffect, useMemo, useState } from "react";
import { api, type Member, type Page, pageLink } from "../../lib/api";
import { memberLink } from "../../lib/mentions";
import { useLive } from "../../lib/useLive";
import { useWorkspace } from "../../lib/workspace";
import { cx, pop } from "../../ui";
import { MemberAvatar } from "../members/MemberAvatar";
import { BLOCKS, type BlockKind, insertBlock } from "./editor/blocks";
import { closeSuggestions, type MenuKey, openSuggestions, type Suggestion } from "./editor/suggestions";

const MAX_PEOPLE_OR_PAGES = 8;
const MENU_MAX_HEIGHT = 320;

const BLOCK_ICONS: Record<BlockKind, ReactNode> = {
  text: <Type size={15} />,
  heading1: <Heading1 size={15} />,
  heading2: <Heading2 size={15} />,
  heading3: <Heading3 size={15} />,
  bullet: <List size={15} />,
  numbered: <ListOrdered size={15} />,
  checklist: <ListChecks size={15} />,
  quote: <Quote size={15} />,
  callout: <MessageSquareWarning size={15} />,
  code: <Code size={15} />,
  table: <Table size={15} />,
  divider: <Minus size={15} />,
};

const LIST_NAMES: Record<Suggestion["kind"], string> = {
  command: "Blocks",
  member: "People to mention",
  page: "Pages to link",
};

type Option = {
  key: string;
  label: string;
  icon: ReactNode;
  /** Shown faintly after the label: the markdown a block writes, or a page's parent. */
  detail: string;
  choose: (view: EditorView, suggestion: Suggestion) => void;
};

/** Whether every word typed starts some word of `text`, so "head 2" finds "Heading 2". */
function matches(text: string, query: string): boolean {
  const words = text.toLowerCase().split(/\s+/);
  return query
    .toLowerCase()
    .split(/\s+/)
    .filter(Boolean)
    .every((wanted) => words.some((word) => word.startsWith(wanted)));
}

function replaceTrigger(view: EditorView, suggestion: Suggestion, insert: string) {
  view.dispatch({
    changes: { from: suggestion.from, to: suggestion.to, insert },
    selection: { anchor: suggestion.from + insert.length },
    effects: closeSuggestions.of(null),
    userEvent: "input.complete",
  });
}

function commandOptions(query: string): Option[] {
  const blocks: Option[] = BLOCKS.filter((block) => matches(`${block.label} ${block.keywords}`, query)).map(
    (block) => ({
      key: block.kind,
      label: block.label,
      icon: BLOCK_ICONS[block.kind],
      detail: block.hint,
      choose: (view, suggestion) => insertBlock(view, suggestion.from, suggestion.to, block.kind),
    }),
  );
  if (!matches("Link to page mention", query)) return blocks;
  return [
    ...blocks,
    {
      key: "page-link",
      label: "Link to page",
      icon: <FileText size={15} />,
      detail: "[[",
      choose: (view, suggestion) =>
        view.dispatch({
          changes: { from: suggestion.from, to: suggestion.to, insert: "[[" },
          selection: { anchor: suggestion.from + 2 },
          effects: openSuggestions.of({ kind: "page", from: suggestion.from }),
          userEvent: "input.complete",
        }),
    },
  ];
}

function memberOptions(query: string, members: Member[], me: string): Option[] {
  return members
    .filter((member) => member.active && member.name && matches(member.name, query))
    .slice(0, MAX_PEOPLE_OR_PAGES)
    .map((member) => ({
      key: member.id,
      label: member.name,
      icon: <MemberAvatar id={member.id} name={member.name} isBot={member.is_bot} size={20} />,
      detail: member.id === me ? "you" : member.is_bot ? "agent" : "",
      choose: (view, suggestion) => replaceTrigger(view, suggestion, `${memberLink(member.name, member.id)} `),
    }));
}

function pageOptions(query: string, pages: Page[], currentPage: string): Option[] {
  const wanted = query.trim().toLowerCase();
  return pages
    .filter((page) => page.id !== currentPage && (page.title || "Untitled").toLowerCase().includes(wanted))
    .sort((left, right) => right.updated_ms - left.updated_ms)
    .slice(0, MAX_PEOPLE_OR_PAGES)
    .map((page) => {
      const title = page.title || "Untitled";
      return {
        key: page.id,
        label: title,
        icon: <FileText size={15} />,
        detail: pages.find((candidate) => candidate.id === page.parent)?.title ?? "",
        // Titles come from other members: kept out of the markdown, they can't
        // break the link. Its chip draws the page's current title.
        choose: (view, suggestion) => replaceTrigger(view, suggestion, `[](${pageLink(page.id)}) `),
      };
    });
}

/**
 * The menu under the cursor for `/`, `@` and `[[`. The editor keeps focus
 * and forwards the menu keys through `keys`; the selected option is its
 * `aria-activedescendant`, so screen readers follow along.
 */
export function SuggestionMenu({
  view,
  suggestion,
  pageId,
  keys,
}: {
  view: EditorView | null;
  suggestion: Suggestion | null;
  pageId: string;
  keys: RefObject<(key: MenuKey, view: EditorView) => boolean>;
}) {
  const { pages, session } = useWorkspace();
  const loadMembers = useCallback(() => api.members(), []);
  const members = useLive(loadMembers, (change) => change.kind === "members") ?? [];
  const [selected, setSelected] = useState(0);
  const listId = useId();

  const kind = suggestion?.kind;
  const query = suggestion?.query ?? "";
  const options = useMemo(() => {
    if (kind === "command") return commandOptions(query);
    if (kind === "member") return memberOptions(query, members, session.me);
    if (kind === "page") return pageOptions(query, pages, pageId);
    return [];
  }, [kind, query, members, session.me, pages, pageId]);

  // A new query starts at the top of the list.
  const [shownFor, setShownFor] = useState({ kind, query });
  if (shownFor.kind !== kind || shownFor.query !== query) {
    setShownFor({ kind, query });
    setSelected(0);
  }
  const selectedIndex = Math.min(selected, Math.max(options.length - 1, 0));
  const isOpen = view !== null && suggestion !== null && options.length > 0;

  useLayoutEffect(() => {
    keys.current = (key, editor) => {
      if (!suggestion) return false;
      if (key === "Escape") {
        editor.dispatch({ effects: closeSuggestions.of(null) });
        return true;
      }
      if (options.length === 0) return false;
      if (key === "ArrowDown" || key === "ArrowUp") {
        const step = key === "ArrowDown" ? 1 : -1;
        setSelected((selectedIndex + step + options.length) % options.length);
        return true;
      }
      options[selectedIndex].choose(editor, suggestion);
      return true;
    };
  });

  useLayoutEffect(() => {
    if (!view) return;
    const content = view.contentDOM;
    if (!isOpen) return;
    content.setAttribute("aria-autocomplete", "list");
    content.setAttribute("aria-expanded", "true");
    content.setAttribute("aria-controls", listId);
    content.setAttribute("aria-activedescendant", `${listId}-${selectedIndex}`);
    document.getElementById(`${listId}-${selectedIndex}`)?.scrollIntoView({ block: "nearest" });
    return () => {
      for (const name of ["aria-autocomplete", "aria-expanded", "aria-controls", "aria-activedescendant"]) {
        content.removeAttribute(name);
      }
    };
  }, [view, isOpen, listId, selectedIndex]);

  if (!isOpen) return null;
  const caret = view.coordsAtPos(suggestion.from);
  if (!caret) return null;
  const frame = view.dom.getBoundingClientRect();
  const isAbove = caret.bottom + MENU_MAX_HEIGHT + 16 > window.innerHeight && caret.top > MENU_MAX_HEIGHT + 16;

  return (
    <motion.div
      {...pop}
      id={listId}
      role="listbox"
      aria-label={LIST_NAMES[suggestion.kind]}
      style={{
        left: Math.max(caret.left - frame.left - 8, 0),
        top: isAbove ? caret.top - frame.top - 6 : caret.bottom - frame.top + 6,
        y: isAbove ? "-100%" : 0,
        maxHeight: MENU_MAX_HEIGHT,
      }}
      className="absolute z-30 w-64 overflow-y-auto rounded-xl border border-line bg-surface p-1 shadow-float"
    >
      {options.map((option, index) => (
        <div
          key={option.key}
          id={`${listId}-${index}`}
          role="option"
          // Focus stays in the editor, which points here through aria-activedescendant.
          tabIndex={-1}
          aria-selected={index === selectedIndex}
          onMouseDown={(event) => {
            event.preventDefault();
            option.choose(view, suggestion);
          }}
          onMouseMove={() => setSelected(index)}
          className={cx(
            "flex h-9 cursor-default items-center gap-2.5 rounded-lg px-2.5 text-sm text-ink",
            index === selectedIndex && "bg-surface-2",
          )}
        >
          <span className="grid w-5 shrink-0 place-items-center text-muted">{option.icon}</span>
          <span className="truncate font-medium">{option.label}</span>
          {option.detail && (
            <span
              className={cx(
                "ml-auto shrink-0 truncate pl-2 text-xs text-muted",
                suggestion.kind === "command" && "font-mono",
              )}
            >
              {option.detail}
            </span>
          )}
        </div>
      ))}
    </motion.div>
  );
}
