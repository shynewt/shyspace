import { ChevronRight, Plus } from "lucide-react";
import { AnimatePresence, motion } from "motion/react";
import { useState } from "react";
import { api } from "../../lib/api";
import { reportError } from "../../lib/toast";
import { useWorkspace } from "../../lib/workspace";
import { IconButton, springs } from "../../ui";
import { PageTree } from "./PageTree";

/** The sidebar's pages, as a tree under a header that folds like a channel section. */
export function PagesSection() {
  const { pages, navigate } = useWorkspace();
  const [isOpen, setOpen] = useState(true);

  const createPage = async (parent: string | null) => {
    try {
      const id = await api.createPage("", parent);
      setOpen(true);
      navigate({ kind: "page", id });
    } catch (error) {
      reportError(error);
    }
  };

  return (
    <section aria-label="Pages" className="mt-3">
      <div className="group/section flex h-7 items-center gap-0.5 pr-1 pl-1">
        <button
          type="button"
          aria-expanded={isOpen}
          onClick={() => setOpen(!isOpen)}
          className="flex min-w-0 flex-1 items-center gap-1 rounded px-1 text-2xs font-semibold tracking-wide text-faint uppercase hover:text-muted"
        >
          <motion.span animate={{ rotate: isOpen ? 90 : 0 }} transition={springs.snappy} className="grid">
            <ChevronRight size={12} />
          </motion.span>
          <span className="truncate">Pages</span>
        </button>
        <IconButton
          label="New page"
          size="sm"
          icon={<Plus size={14} />}
          onClick={() => createPage(null)}
          className="opacity-0 group-hover/section:opacity-100 focus-visible:opacity-100"
        />
      </div>
      <AnimatePresence initial={false}>
        {isOpen && (
          <motion.div
            initial={{ height: 0, opacity: 0 }}
            animate={{ height: "auto", opacity: 1 }}
            exit={{ height: 0, opacity: 0 }}
            transition={{ duration: 0.18, ease: "easeOut" }}
            className="flex flex-col gap-px overflow-hidden"
          >
            <PageTree pages={pages} parent={null} depth={0} onCreateChild={createPage} />
          </motion.div>
        )}
      </AnimatePresence>
    </section>
  );
}
