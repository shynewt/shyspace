import { AlarmClock, Bookmark, Check, ChevronRight, Ellipsis, FileText, Trash2, Undo2 } from "lucide-react";
import { AnimatePresence, motion } from "motion/react";
import { useCallback, useMemo, useRef, useState } from "react";
import { api, type LaterItem } from "../../lib/api";
import { plainPreview } from "../../lib/markdown";
import { displayName } from "../../lib/people";
import { describeReminder } from "../../lib/reminders";
import { reportError, showUndoToast } from "../../lib/toast";
import { useContextMenu } from "../../lib/useContextMenu";
import { useLive } from "../../lib/useLive";
import { savedItemView, useWorkspace } from "../../lib/workspace";
import { Avatar, cx, EmptyState, IconButton, Menu, type MenuItem, SectionLabel, springs } from "../../ui";
import { ChannelName } from "../private/ChannelName";
import { ReminderPicker } from "./ReminderPicker";
import { remindMeItem } from "./remindMeItem";

/** Messages and pages you saved for yourself, with their reminders. */
export function LaterView() {
  const items = useLive(api.later, (change) => change.kind === "later" || change.kind === "messages");
  const [isCompletedOpen, setCompletedOpen] = useState(false);
  const open = items?.filter((item) => !item.is_done) ?? [];
  const done = items?.filter((item) => item.is_done) ?? [];

  return (
    <div className="flex h-full flex-col">
      <header className="flex h-13 shrink-0 items-center gap-2 border-b border-line px-5">
        <Bookmark size={17} className="text-decorative" />
        <h1 className="text-[15px] font-semibold">Later</h1>
      </header>
      <div className="flex-1 overflow-y-auto px-5 py-4">
        <div className="mx-auto max-w-[760px]">
          {items && items.length === 0 && (
            <EmptyState icon={<Bookmark size={22} />} title="Nothing saved for later">
              Save a message or page from its menu, with a reminder if you like. Only you see this list.
            </EmptyState>
          )}
          <ul aria-label="Saved for later" className="flex flex-col gap-1">
            <AnimatePresence initial={false}>
              {open.map((item) => (
                <LaterRow key={item.id} item={item} />
              ))}
            </AnimatePresence>
          </ul>
          {done.length > 0 && (
            <section className="mt-5">
              <button
                type="button"
                aria-expanded={isCompletedOpen}
                onClick={() => setCompletedOpen(!isCompletedOpen)}
                className="flex items-center gap-1 rounded-md text-left"
              >
                <motion.span
                  animate={{ rotate: isCompletedOpen ? 90 : 0 }}
                  transition={springs.snappy}
                  className="grid text-decorative"
                >
                  <ChevronRight size={14} />
                </motion.span>
                <SectionLabel>Completed · {done.length}</SectionLabel>
              </button>
              <AnimatePresence initial={false}>
                {isCompletedOpen && (
                  <motion.ul
                    aria-label="Completed"
                    initial={{ height: 0, opacity: 0 }}
                    animate={{ height: "auto", opacity: 1 }}
                    exit={{ height: 0, opacity: 0 }}
                    className="flex flex-col gap-1 overflow-hidden"
                  >
                    {done.map((item) => (
                      <LaterRow key={item.id} item={item} />
                    ))}
                  </motion.ul>
                )}
              </AnimatePresence>
            </section>
          )}
        </div>
      </div>
    </div>
  );
}

function LaterRow({ item }: { item: LaterItem }) {
  const { pages, navigate } = useWorkspace();
  const row = useRef<HTMLLIElement>(null);
  const [isMenuOpen, setMenuOpen] = useState(false);
  const openMenu = useCallback(() => setMenuOpen(true), []);
  useContextMenu(row, openMenu);
  const titles = useMemo(() => new Map(pages.map((page) => [page.id, page.title || "Untitled"])), [pages]);
  const linked = item.message;
  const openItem = () => {
    const view = savedItemView(item);
    if (view) navigate(view);
  };
  const author = linked ? displayName(linked.message.author_name, linked.message.author) : null;

  const preview = linked ? plainPreview(linked.message.text, (id) => titles.get(id)) : item.page?.title || "Untitled";
  const isOverdue = item.remind_ms !== null && item.remind_ms <= Date.now() && !item.is_done;

  const setDone = (isDone: boolean) =>
    api
      .markLaterDone(item.id, isDone)
      .then(() => isDone && showUndoToast("Marked as done.", () => api.markLaterDone(item.id, false)))
      .catch(reportError);
  const [isPicking, setPicking] = useState(false);
  const remind = (at: number | null) => api.saveForLater(item.id, at).catch(reportError);
  const remove = () =>
    api
      .removeFromLater(item.id)
      .then(() => showUndoToast("Removed from Later.", () => api.saveForLater(item.id, item.remind_ms)))
      .catch(reportError);
  const menu: MenuItem[] = [
    remindMeItem({
      remindMs: item.is_done ? null : item.remind_ms,
      onRemind: remind,
      onCustom: () => setPicking(true),
    }),
    {
      label: item.is_done ? "Mark as not done" : "Mark as done",
      icon: item.is_done ? <Undo2 size={14} /> : <Check size={14} />,
      onSelect: () => setDone(!item.is_done),
    },
    { label: "Remove from Later", icon: <Trash2 size={14} />, onSelect: remove, isDestructive: true },
  ];

  return (
    <motion.li
      ref={row}
      layout
      initial={{ opacity: 0, y: 4 }}
      animate={{ opacity: 1, y: 0 }}
      exit={{ opacity: 0, x: 24, height: 0, transition: { duration: 0.22, ease: "easeIn" } }}
      transition={springs.smooth}
      className={cx(
        "group flex items-center gap-3 rounded-xl px-2.5 py-2 hover:bg-surface-2",
        isMenuOpen && "bg-surface-2",
      )}
    >
      {linked ? (
        <Avatar id={linked.message.author} name={author ?? ""} isBot={linked.message.author_is_bot} size={30} />
      ) : (
        <span
          aria-hidden
          className="grid size-[30px] shrink-0 place-items-center rounded-[30%] bg-surface-2 text-muted"
        >
          <FileText size={15} />
        </span>
      )}
      <button type="button" onClick={openItem} className="min-w-0 flex-1 text-left">
        <span className="flex items-center gap-1.5 text-[12.5px] text-muted">
          {linked ? (
            <span className="flex min-w-0 items-center gap-1">
              <span className="shrink-0">{author} in </span>
              <ChannelName channelId={linked.message.channel} className="min-w-0" />
            </span>
          ) : (
            <span>Page</span>
          )}
          {item.remind_ms !== null && !item.is_done && (
            <span
              className={cx(
                "flex shrink-0 items-center gap-1 rounded-full px-2 text-[11px]",
                isOverdue ? "bg-danger/15 text-danger-text" : "bg-accent-soft text-accent-text",
              )}
            >
              <AlarmClock size={11} /> {describeReminder(item.remind_ms, new Date())}
            </span>
          )}
        </span>
        <span className={cx("mt-0.5 block truncate text-[13.5px]", item.is_done && "text-faint line-through")}>
          {preview}
        </span>
      </button>
      <span
        className={cx(
          "flex shrink-0 items-center gap-0.5 opacity-0 transition-opacity group-hover:opacity-100 keyboard-focus-within:opacity-100",
          isMenuOpen && "opacity-100",
        )}
      >
        <IconButton
          label={item.is_done ? "Mark as not done" : "Mark as done"}
          size="sm"
          icon={item.is_done ? <Undo2 size={14} /> : <Check size={14} />}
          onClick={() => setDone(!item.is_done)}
        />
        <Menu
          open={isMenuOpen}
          onOpenChange={setMenuOpen}
          trigger={<IconButton label="Later actions" size="sm" icon={<Ellipsis size={14} />} />}
          items={menu}
        />
      </span>
      <ReminderPicker open={isPicking} onOpenChange={setPicking} initialMs={item.remind_ms} onPick={remind} />
    </motion.li>
  );
}
