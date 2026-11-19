import { useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { AnimatePresence, motion } from "motion/react";
import { CornerDownLeft, FileText, Hash, MessageSquare, Search } from "lucide-react";
import { api, type SearchHit } from "../lib/api";
import { reportError } from "../lib/toast";
import { useWorkspace, type View } from "../lib/workspace";

const SEARCH_DELAY_MS = 120;

type Item = { key: string; view: View; icon: ReactNode; title: string; detail?: ReactNode; group: string };

export function CommandPalette({ open, onClose }: { open: boolean; onClose: () => void }) {
  return (
    <AnimatePresence>
      {open && (
        <motion.div
          className="fixed inset-0 z-40 flex items-start justify-center bg-black/25 pt-[14vh] backdrop-blur-[2px]"
          initial={{ opacity: 0 }}
          animate={{ opacity: 1 }}
          exit={{ opacity: 0, transition: { duration: 0.12 } }}
          onMouseDown={onClose}
        >
          <motion.div
            className="w-[600px] max-w-[92vw] overflow-hidden rounded-2xl border border-line bg-surface shadow-float"
            initial={{ opacity: 0, scale: 0.96, y: -8 }}
            animate={{ opacity: 1, scale: 1, y: 0 }}
            exit={{ opacity: 0, scale: 0.97, y: -4, transition: { duration: 0.1 } }}
            onMouseDown={(event) => event.stopPropagation()}
          >
            <PaletteBody onClose={onClose} />
          </motion.div>
        </motion.div>
      )}
    </AnimatePresence>
  );
}

function PaletteBody({ onClose }: { onClose: () => void }) {
  const { channels, pages, navigate } = useWorkspace();
  const [query, setQuery] = useState("");
  const [hits, setHits] = useState<SearchHit[]>([]);
  const [selected, setSelected] = useState(0);
  const list = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!query.trim()) return setHits([]);
    const timer = setTimeout(() => api.search(query).then(setHits, reportError), SEARCH_DELAY_MS);
    return () => clearTimeout(timer);
  }, [query]);

  const items = useMemo<Item[]>(() => {
    const needle = query.trim().toLowerCase();
    const matches = (name: string) => !needle || name.toLowerCase().includes(needle);
    const jumpTo: Item[] = [
      ...channels.filter((channel) => matches(channel.name)).map((channel) => ({
        key: `channel:${channel.id}`,
        view: { kind: "channel", id: channel.id } as View,
        icon: <Hash size={15} />,
        title: channel.name,
        group: "Jump to",
      })),
      ...pages.filter((page) => matches(page.title || "Untitled")).map((page) => ({
        key: `page:${page.id}`,
        view: { kind: "page", id: page.id } as View,
        icon: <FileText size={15} />,
        title: page.title || "Untitled",
        group: "Jump to",
      })),
    ].slice(0, needle ? 6 : 12);
    const found: Item[] = hits.flatMap((hit): Item[] => {
      if (hit.kind === "page") {
        return [{ key: `hit:${hit.id}`, view: { kind: "page", id: hit.id }, icon: <FileText size={15} />, title: hit.title || "Untitled", detail: <Snippet text={hit.snippet} />, group: "In pages" }];
      }
      if (!hit.channel) return [];
      return [{ key: `hit:${hit.id}`, view: { kind: "channel", id: hit.channel }, icon: <MessageSquare size={15} />, title: `#${hit.title}`, detail: <Snippet text={hit.snippet} />, group: "In messages" }];
    });
    return [...jumpTo, ...found];
  }, [query, hits, channels, pages]);

  useEffect(() => setSelected(0), [query]);
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
          autoFocus
          value={query}
          onChange={(event) => setQuery(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === "ArrowDown") {
              event.preventDefault();
              setSelected((index) => Math.min(index + 1, items.length - 1));
            } else if (event.key === "ArrowUp") {
              event.preventDefault();
              setSelected((index) => Math.max(index - 1, 0));
            } else if (event.key === "Enter") {
              choose(items[selected]);
            } else if (event.key === "Escape") {
              onClose();
            }
          }}
          placeholder="Search messages and pages, or jump to…"
          className="h-13 flex-1 bg-transparent text-[15px] outline-none placeholder:text-faint"
        />
      </div>
      <div ref={list} className="max-h-[420px] overflow-y-auto p-2">
        {items.length === 0 && <p className="px-3 py-8 text-center text-[13px] text-muted">Nothing found for “{query}”</p>}
        {items.map((item, index) => (
          <div key={item.key}>
            {(index === 0 || items[index - 1].group !== item.group) && (
              <div className="px-3 pt-2 pb-1 text-[11px] font-semibold tracking-wide text-faint uppercase">{item.group}</div>
            )}
            <button
              data-index={index}
              onMouseMove={() => setSelected(index)}
              onClick={() => choose(item)}
              className="relative flex w-full items-start gap-3 rounded-lg px-3 py-2 text-left"
            >
              {selected === index && (
                <motion.div layoutId="palette-selection" className="absolute inset-0 rounded-lg bg-surface-2" transition={{ type: "spring", stiffness: 700, damping: 45 }} />
              )}
              <span className={`relative mt-0.5 ${selected === index ? "text-accent" : "text-faint"}`}>{item.icon}</span>
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

/** The core marks matches with \u0001…\u0002; render them as highlights. */
function Snippet({ text }: { text: string }) {
  const parts = text.split(/(\u0001[^\u0002]*\u0002)/);
  return (
    <>
      {parts.map((part, index) =>
        part.startsWith("\u0001") ? (
          <mark key={index} className="hit">
            {part.slice(1, -1)}
          </mark>
        ) : (
          <span key={index}>{part}</span>
        ),
      )}
    </>
  );
}
