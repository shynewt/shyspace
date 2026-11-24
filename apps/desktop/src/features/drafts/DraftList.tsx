import { Pencil, Trash2 } from "lucide-react";
import { AnimatePresence, motion, type Transition } from "motion/react";
import { type KeyboardEvent, useMemo } from "react";
import { api, type Draft } from "../../lib/api";
import { plainPreview } from "../../lib/markdown";
import { useMemberNames } from "../../lib/members";
import { reportError, showUndoToast } from "../../lib/toast";
import { useLive } from "../../lib/useLive";
import { useWorkspace } from "../../lib/workspace";
import { Badge, EmptyState, IconButton, instantAfterAKey, ListRow, RelativeTime, tweens } from "../../ui";
import { ChannelName } from "../private/ChannelName";

const OLD_AFTER_MS = 7 * 24 * 60 * 60 * 1000;

/** Unsent text, one row per conversation or thread, last edited first. */
export function DraftList() {
  const drafts = useLive(api.drafts, (change) => change.kind === "drafts");
  if (!drafts) return null;
  if (drafts.length === 0) {
    return (
      <EmptyState icon={<Pencil size={22} />} title="No drafts">
        Anything you start typing and leave is kept here, on this device only.
      </EmptyState>
    );
  }
  return (
    <div className="flex flex-col gap-3">
      <ul aria-label="Drafts" className="flex flex-col gap-1">
        {/* A row discarded with a key leaves at once; the exit reads this when it starts. */}
        <AnimatePresence initial={false} custom={instantAfterAKey(tweens.base)}>
          {drafts.map((draft) => (
            <DraftRow key={draft.target} draft={draft} />
          ))}
        </AnimatePresence>
      </ul>
      {/* Drafts never leave the device, so nobody else (nor the bucket) sees half-written text. */}
      <p className="px-3 text-xs text-muted">Drafts stay on this device.</p>
    </div>
  );
}

const rowMotion = {
  initial: { opacity: 0, y: 4 },
  animate: { opacity: 1, y: 0 },
  exit: (transition: Transition) => ({ opacity: 0, height: 0, transition }),
};

function DraftRow({ draft }: { draft: Draft }) {
  const { navigate, pages } = useWorkspace();
  const titles = useMemo(() => new Map(pages.map((page) => [page.id, page.title || "Untitled"])), [pages]);
  const memberName = useMemberNames();
  const isThread = draft.target !== draft.channel;
  const isOld = Date.now() - draft.updated_ms > OLD_AFTER_MS;
  const open = () => navigate({ kind: "channel", id: draft.channel, thread: isThread ? draft.target : undefined });
  const discard = () =>
    api
      .setDraft(draft.target, "")
      .then(() => showUndoToast("Draft discarded.", () => api.setDraft(draft.target, draft.text)))
      .catch(reportError);

  const discardWithKey = (event: KeyboardEvent<HTMLLIElement>) => {
    if (event.key !== "Delete" && event.key !== "Backspace") return;
    if (!(event.target instanceof HTMLElement) || !event.target.matches("[data-list-row]")) return;
    event.preventDefault();
    moveFocusOffRow(event.target);
    discard();
  };

  return (
    <motion.li variants={rowMotion} initial="initial" animate="animate" exit="exit" onKeyDown={discardWithKey}>
      <ListRow
        onClick={open}
        leading={<Pencil size={15} className="mt-0.5 text-decorative" />}
        meta={
          <>
            {isThread && <span className="shrink-0">Thread in</span>}
            <ChannelName channelId={draft.channel} className="min-w-0 font-semibold text-ink" />
            {isOld && <Badge>Old</Badge>}
            <RelativeTime ms={draft.updated_ms} className="ml-auto shrink-0 text-faint" />
          </>
        }
        trailing={
          <span className="flex shrink-0 opacity-0 transition-opacity group-hover:opacity-100 keyboard-focus-within:opacity-100">
            <IconButton label="Discard draft" size="sm" icon={<Trash2 size={14} />} onClick={discard} />
          </span>
        }
      >
        {plainPreview(draft.text, (id) => titles.get(id), memberName)}
      </ListRow>
    </motion.li>
  );
}

/** Moves focus off a row that's going: to the next row, else the one before, else the chosen tab. */
function moveFocusOffRow(row: HTMLElement) {
  const rows = [...(row.closest("ul")?.querySelectorAll<HTMLElement>("[data-list-row]") ?? [])];
  const index = rows.indexOf(row);
  const neighbour =
    rows[index + 1] ?? rows[index - 1] ?? document.querySelector<HTMLElement>('[role="tab"][aria-selected="true"]');
  neighbour?.focus();
}
