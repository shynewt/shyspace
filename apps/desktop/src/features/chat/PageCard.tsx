import { ArrowUpRight, FileText } from "lucide-react";
import { motion } from "motion/react";
import { useCallback, useMemo } from "react";
import { api } from "../../lib/api";
import { useLive } from "../../lib/useLive";
import { useWorkspace } from "../../lib/workspace";
import { tweens } from "../../ui";
import { pagePreview } from "../pages/pageText";

/** A page shared in chat. It stays live: edits to the page show up here. */
export function PageCard({ pageId }: { pageId: string }) {
  const { pages, navigate } = useWorkspace();
  const page = pages.find((candidate) => candidate.id === pageId);
  const loadState = useCallback(() => api.pageState(pageId), [pageId]);
  const state = useLive(
    loadState,
    (change) => (change.kind === "doc" || change.kind === "page_reset") && change.page === pageId,
  );
  const preview = useMemo(() => (state ? pagePreview(state) : ""), [state]);
  if (!page) return null;
  return (
    <motion.button
      type="button"
      whileHover={{ y: -2 }}
      transition={tweens.fast}
      onClick={() => navigate({ kind: "page", id: pageId })}
      className="group/card mt-1.5 flex w-full max-w-card items-start gap-3 rounded-xl border border-line bg-surface p-3 text-left shadow-card transition-colors hover:border-accent/40"
    >
      <div className="grid h-9 w-9 shrink-0 place-items-center rounded-lg bg-accent-soft text-accent-text">
        <FileText size={17} />
      </div>
      <div className="min-w-0 flex-1">
        <div className="flex items-center gap-1 text-base font-semibold">
          <span className="truncate">{page.title || "Untitled"}</span>
          <ArrowUpRight
            size={14}
            className="shrink-0 text-decorative transition group-hover/card:translate-x-0.5 group-hover/card:-translate-y-0.5 group-hover/card:text-accent"
          />
        </div>
        <p className="mt-0.5 line-clamp-2 text-sm leading-snug text-muted">{preview || "Empty page"}</p>
      </div>
    </motion.button>
  );
}
