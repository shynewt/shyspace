import { CornerDownLeft, FileText, Hash, MessageSquare, Search } from "lucide-react";
import { motion } from "motion/react";
import { type ReactNode, useEffect, useMemo, useRef, useState } from "react";
import { api, type SearchHit } from "../../lib/api";
import { reportError } from "../../lib/toast";
import { useWorkspace, type View } from "../../lib/workspace";
import { Dialog, springs } from "../../ui";

const SEARCH_DELAY_MS = 120;
const JUMP_LIMIT_WHILE_TYPING = 6;
const JUMP_LIMIT_EMPTY = 12;

type Item = { key: string; view: View; icon: ReactNode; title: string; detail?: ReactNode; group: string };

export function CommandPalette({ open, onOpenChange }: { open: boolean; onOpenChange: (open: boolean) => void }) {
  return (
    <Dialog open={open} onOpenChange={onOpenChange} title="Search" hideTitle placement="top" width={600}>
      <PaletteBody onClose={() => onOpenChange(false)} />
    </Dialog>
  );
}

function PaletteBody({ onClose }: { onClose: () => void }) {
  const { channels, pages, navigate } = useWorkspace();
  const [query, setQuery] = useState("");
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
    const needle = query.trim().toLowerCase();
    const matches = (name: string) => !needle || name.toLowerCase().includes(needle);
    const jumpTo: Item[] = [
      ...channels
        .filter((channel) => matches(channel.name))
        .map(
          (channel): Item => ({
            key: `channel:${channel.id}`,
            view: { kind: "channel", id: channel.id },
            icon: <Hash size={15} />,
            title: channel.name,
            group: "Jump to",
          }),
        ),
      ...pages
        .filter((page) => matches(page.title || "Untitled"))
        .map(
          (page): Item => ({
            key: `page:${page.id}`,
            view: { kind: "page", id: page.id },
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
            view: { kind: "page", id: hit.id },
            icon: <FileText size={15} />,
            title: hit.title || "Untitled",
            detail: <Snippet text={hit.snippet} />,
            group: "In pages",
          },
        ];
      }
      if (!hit.channel) return [];
      return [
        {
          key: `hit:${hit.id}`,
          view: { kind: "channel", id: hit.channel },
          icon: <MessageSquare size={15} />,
          title: `#${hit.title}`,
          detail: <Snippet text={hit.snippet} />,
          group: "In messages",
        },
      ];
    });
    return [...jumpTo, ...found];
  }, [query, hits, channels, pages]);

  useEffect(() => {
    list.current?.querySelector(`[data-index="${selected}"]`)?.scrollIntoView({ block: "nearest" });
  }, [selected]);

  const choose = (item: Item | undefined) => {
    if (!item) return;
    navigate(item.view);
    onClose();
  };

  return (
    <>
      <div className="flex items-center gap-3 border-b border-line px-4">
        <Search size={17} className="text-faint" />
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
            if (event.key === "ArrowDown") {
              event.preventDefault();
              setSelected((index) => Math.min(index + 1, items.length - 1));
            } else if (event.key === "ArrowUp") {
              event.preventDefault();
              setSelected((index) => Math.max(index - 1, 0));
            } else if (event.key === "Enter") {
              choose(items[selected]);
            }
          }}
          placeholder="Search messages and pages, or jump to…"
          className="h-13 flex-1 bg-transparent text-[15px] outline-none placeholder:text-faint"
        />
      </div>
      <div id="palette-results" ref={list} role="listbox" className="max-h-[420px] overflow-y-auto p-2">
        {items.length === 0 && (
          <p className="px-3 py-8 text-center text-[13px] text-muted">Nothing found for “{query}”</p>
        )}
        {items.map((item, index) => (
          <div key={item.key}>
            {(index === 0 || items[index - 1].group !== item.group) && (
              <div className="px-3 pt-2 pb-1 text-[11px] font-semibold tracking-wide text-faint uppercase">
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
              <span className={`relative mt-0.5 ${selected === index ? "text-accent" : "text-faint"}`}>
                {item.icon}
              </span>
              <span className="relative min-w-0 flex-1">
                <span className="block truncate text-[13.5px] font-medium">{item.title}</span>
                {item.detail && <span className="mt-0.5 block truncate text-[12.5px] text-muted">{item.detail}</span>}
              </span>
              {selected === index && <CornerDownLeft size={14} className="relative mt-1 text-faint" />}
            </button>
          </div>
        ))}
      </div>
    </>
  );
}

const MATCH_START = String.fromCharCode(1);
const MATCH_END = String.fromCharCode(2);

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
