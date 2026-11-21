import {
  CheckCheck,
  CornerDownLeft,
  FilePlus2,
  FileText,
  Inbox,
  MessageSquare,
  Search,
  Settings,
  UserPlus,
} from "lucide-react";
import { motion } from "motion/react";
import { type ReactNode, useEffect, useMemo, useRef, useState } from "react";
import { api, type SearchHit } from "../../lib/api";
import { useChannelNames } from "../../lib/channels";
import { plainPreview } from "../../lib/markdown";
import { SETTINGS_SECTIONS } from "../../lib/settings";
import { hasCommandModifier, shortcutKeys } from "../../lib/shortcuts";
import { reportError } from "../../lib/toast";
import { useWorkspace, type View } from "../../lib/workspace";
import { Dialog, Kbd, springs } from "../../ui";
import { MarkAllReadDialog } from "../inbox/MarkAllReadDialog";
import { ChannelGlyph } from "../private/ChannelGlyph";
import { ChannelName } from "../private/ChannelName";

const SEARCH_DELAY_MS = 120;
const JUMP_LIMIT_WHILE_TYPING = 6;
const JUMP_LIMIT_EMPTY = 12;

type Item = {
  key: string;
  onChoose: () => void;
  icon: ReactNode;
  title: ReactNode;
  detail?: ReactNode;
  group: string;
  shortcut?: string[];
};

export function CommandPalette({
  open,
  initialQuery,
  onOpenChange,
}: {
  open: boolean;
  /** Typed in as it opens, like `from:"Grace"`. */
  initialQuery: string;
  onOpenChange: (open: boolean) => void;
}) {
  const [isConfirmingMarkAll, setConfirmingMarkAll] = useState(false);
  return (
    <>
      <Dialog
        open={open}
        onOpenChange={onOpenChange}
        title="Search"
        hideTitle
        placement="top"
        width={600}
        padding="none"
      >
        <PaletteBody
          initialQuery={initialQuery}
          onClose={() => onOpenChange(false)}
          onMarkAllRead={() => setConfirmingMarkAll(true)}
        />
      </Dialog>
      <MarkAllReadDialog open={isConfirmingMarkAll} onOpenChange={setConfirmingMarkAll} />
    </>
  );
}

function PaletteBody({
  initialQuery,
  onClose,
  onMarkAllRead,
}: {
  initialQuery: string;
  onClose: () => void;
  onMarkAllRead: () => void;
}) {
  const { channels, pages, navigate, session, openSettings } = useWorkspace();
  const names = useChannelNames();
  const [query, setQuery] = useState(initialQuery);
  const [hits, setHits] = useState<SearchHit[]>([]);
  const [selected, setSelected] = useState(0);
  const list = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!query.trim()) {
      setHits([]);
      return;
    }
    const timer = setTimeout(() => api.search(query).then(setHits, reportError), SEARCH_DELAY_MS);
    return () => clearTimeout(timer);
  }, [query]);

  const items = useMemo<Item[]>(() => {
    const titles = new Map(pages.map((page) => [page.id, page.title || "Untitled"]));
    const snippet = (text: string) => <Snippet text={readableSnippet(text, (id) => titles.get(id))} />;
    const needle = query.trim().toLowerCase();
    const matches = (name: string) => !needle || name.toLowerCase().includes(needle);
    const go = (view: View) => () => navigate(view);
    const actions: Item[] = [
      {
        title: "Open inbox",
        icon: <Inbox size={15} />,
        onChoose: go({ kind: "inbox" }),
        shortcut: shortcutKeys("inbox"),
      },
      {
        title: "New page",
        icon: <FilePlus2 size={15} />,
        onChoose: () => {
          api.createPage("", null).then((id) => navigate({ kind: "page", id }), reportError);
        },
      },
      {
        title: "Mark all as read",
        icon: <CheckCheck size={15} />,
        onChoose: onMarkAllRead,
      },
      ...(session.is_admin
        ? [
            {
              title: "Invite people",
              icon: <UserPlus size={15} />,
              onChoose: () => openSettings({ section: "members", membersTab: "invites" }),
            },
          ]
        : []),
      // Settings sections show only while searching, so the empty palette stays short.
      ...(needle
        ? SETTINGS_SECTIONS.map((section) => ({
            title: `Settings: ${section.label}`,
            icon: <Settings size={15} />,
            onChoose: () => openSettings({ section: section.id }),
          }))
        : []),
    ]
      .filter((action) => matches(action.title))
      .map((action) => ({ ...action, key: `action:${action.title}`, group: "Actions" }));
    const jumpTo: Item[] = [
      ...channels
        .filter((channel) => matches(names.title(channel.id)))
        .map(
          (channel): Item => ({
            key: `channel:${channel.id}`,
            onChoose: go({ kind: "channel", id: channel.id }),
            icon: <ChannelGlyph kind={channel.kind} />,
            title: names.title(channel.id),
            group: "Jump to",
          }),
        ),
      ...pages
        .filter((page) => matches(page.title || "Untitled"))
        .map(
          (page): Item => ({
            key: `page:${page.id}`,
            onChoose: go({ kind: "page", id: page.id }),
            icon: <FileText size={15} />,
            title: page.title || "Untitled",
            group: "Jump to",
          }),
        ),
    ].slice(0, needle ? JUMP_LIMIT_WHILE_TYPING : JUMP_LIMIT_EMPTY);
    const found = hits.flatMap((hit): Item[] => {
      if (hit.kind === "page") {
        return [
          {
            key: `hit:${hit.id}`,
            onChoose: go({ kind: "page", id: hit.id }),
            icon: <FileText size={15} />,
            title: hit.title || "Untitled",
            detail: snippet(hit.snippet),
            group: "In pages",
          },
        ];
      }
      if (!hit.channel) return [];
      return [
        {
          key: `hit:${hit.id}`,
          onChoose: go({ kind: "channel", id: hit.channel, thread: hit.thread_root ?? undefined, focus: hit.id }),
          icon: <MessageSquare size={15} />,
          title: <ChannelName channelId={hit.channel} />,
          detail: snippet(hit.snippet),
          group: "In messages",
        },
      ];
    });
    return [...jumpTo, ...found, ...actions];
  }, [query, hits, channels, names, pages, navigate, session.is_admin, onMarkAllRead, openSettings]);

  useEffect(() => {
    list.current?.querySelector(`[data-index="${selected}"]`)?.scrollIntoView({ block: "nearest" });
  }, [selected]);

  const choose = (item: Item | undefined) => {
    if (!item) return;
    item.onChoose();
    onClose();
  };

  return (
    <>
      <div className="flex items-center gap-3 border-b border-line px-4">
        <Search size={17} className="text-decorative" />
        <input
          value={query}
          aria-label="Search messages and pages"
          role="combobox"
          aria-expanded="true"
          aria-controls="palette-results"
          onChange={(event) => {
            setQuery(event.target.value);
            setSelected(0);
          }}
          onKeyDown={(event) => {
            const isMove = !hasCommandModifier(event);
            if (event.key === "ArrowDown" && isMove) {
              event.preventDefault();
              setSelected((index) => Math.min(index + 1, items.length - 1));
            } else if (event.key === "ArrowUp" && isMove) {
              event.preventDefault();
              setSelected((index) => Math.max(index - 1, 0));
            } else if (event.key === "Enter") {
              choose(items[selected]);
            }
          }}
          placeholder="Search, jump to, or run an action…"
          className="h-13 flex-1 bg-transparent text-lg outline-none placeholder:text-faint"
        />
      </div>
      {!query && (
        <p className="border-b border-line px-4 py-1.5 text-xs text-faint">
          Filters: <code>from:name</code> <code>in:#channel</code> <code>before:2026-11-19</code>{" "}
          <code>after:2026-11-01</code>
        </p>
      )}
      <div id="palette-results" ref={list} role="listbox" className="max-h-[420px] overflow-y-auto p-2">
        {items.length === 0 && <p className="px-3 py-8 text-center text-sm text-muted">Nothing found for “{query}”</p>}
        {items.map((item, index) => (
          <div key={item.key}>
            {(index === 0 || items[index - 1].group !== item.group) && (
              <div className="px-3 pt-2 pb-1 text-2xs font-semibold tracking-wide text-faint uppercase">
                {item.group}
              </div>
            )}
            <button
              type="button"
              role="option"
              aria-selected={selected === index}
              data-index={index}
              onMouseMove={() => setSelected(index)}
              onClick={() => choose(item)}
              className="relative flex w-full items-start gap-3 rounded-lg px-3 py-2 text-left"
            >
              {selected === index && (
                <motion.div
                  layoutId="palette-selection"
                  className="absolute inset-0 rounded-lg bg-surface-2"
                  transition={springs.snappy}
                />
              )}
              <span className={`relative mt-0.5 ${selected === index ? "text-accent" : "text-decorative"}`}>
                {item.icon}
              </span>
              <span className="relative min-w-0 flex-1">
                <span className="block truncate text-base font-medium">{item.title}</span>
                {item.detail && <span className="mt-0.5 block truncate text-sm text-muted">{item.detail}</span>}
              </span>
              {item.shortcut && (
                <span className="relative mt-0.5 flex gap-1">
                  {item.shortcut.map((key) => (
                    <Kbd key={key}>{key}</Kbd>
                  ))}
                </span>
              )}
              {selected === index && <CornerDownLeft size={14} className="relative mt-1 text-decorative" />}
            </button>
          </div>
        ))}
      </div>
    </>
  );
}

const MATCH_START = String.fromCharCode(1);
const MATCH_END = String.fromCharCode(2);
const MATCH_MARKS = new RegExp(`[${MATCH_START}${MATCH_END}]`, "g");
/** An app link like `shy://page/<id>`, with or without match marks inside it. */
const MARKED_APP_LINK = new RegExp(`${MATCH_START}?shy${MATCH_END}?://[^\\s)\\]]*`, "g");

/** The snippet as plain words, keeping the match marks that fall on visible text. */
function readableSnippet(text: string, pageTitle: (id: string) => string | undefined): string {
  const withPlainLinks = text.replace(MARKED_APP_LINK, (link) => link.replace(MATCH_MARKS, ""));
  return plainPreview(withPlainLinks, pageTitle);
}

/** The core wraps matches in \u0001…\u0002; render them as highlights. */
function Snippet({ text }: { text: string }) {
  const parts: Array<{ start: number; text: string; isMatch: boolean }> = [];
  let offset = 0;
  for (const chunk of text.split(MATCH_START)) {
    const end = chunk.indexOf(MATCH_END);
    if (end === -1) {
      parts.push({ start: offset, text: chunk, isMatch: false });
    } else {
      parts.push({ start: offset, text: chunk.slice(0, end), isMatch: true });
      parts.push({ start: offset + end + 1, text: chunk.slice(end + 1), isMatch: false });
    }
    offset += chunk.length + 1;
  }
  return (
    <>
      {parts.map((part) =>
        part.isMatch ? (
          <mark key={part.start} className="hit">
            {part.text}
          </mark>
        ) : (
          <span key={part.start}>{part.text}</span>
        ),
      )}
    </>
  );
}
