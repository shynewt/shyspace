import { ChevronRight, FileText, Plus } from "lucide-react";
import { AnimatePresence, motion } from "motion/react";
import { useMemo, useState } from "react";
import type { Page } from "../../lib/api";
import { IconButton, springs } from "../../ui";
import { NavItem } from "./NavItem";

export function PageTree({
  pages,
  parent,
  depth,
  onCreateChild,
}: {
  pages: Page[];
  parent: string | null;
  depth: number;
  onCreateChild: (parent: string) => void;
}) {
  const children = useMemo(() => pages.filter((page) => page.parent === parent), [pages, parent]);
  return (
    <>
      {children.map((page) => (
        <PageNode key={page.id} page={page} pages={pages} depth={depth} onCreateChild={onCreateChild} />
      ))}
    </>
  );
}

function PageNode({
  page,
  pages,
  depth,
  onCreateChild,
}: {
  page: Page;
  pages: Page[];
  depth: number;
  onCreateChild: (parent: string) => void;
}) {
  const [isOpen, setOpen] = useState(true);
  const hasChildren = pages.some((candidate) => candidate.parent === page.id);
  const title = page.title || "Untitled";
  return (
    <div>
      <NavItem
        view={{ kind: "page", id: page.id }}
        depth={depth}
        label={title}
        icon={<FileText size={15} />}
        leading={
          hasChildren && (
            <button
              type="button"
              aria-expanded={isOpen}
              aria-label={`${isOpen ? "Collapse" : "Expand"} ${title}`}
              onClick={() => setOpen(!isOpen)}
              className="relative -mr-1 grid h-6 w-5 shrink-0 place-items-center rounded text-faint hover:text-ink"
            >
              <motion.span animate={{ rotate: isOpen ? 90 : 0 }} transition={springs.snappy} className="grid">
                <ChevronRight size={14} />
              </motion.span>
            </button>
          )
        }
        trailing={
          <IconButton
            label="New sub-page"
            size="sm"
            icon={<Plus size={13} />}
            onClick={() => onCreateChild(page.id)}
            className="absolute right-1.5 opacity-0 group-hover:opacity-100 focus-visible:opacity-100"
          />
        }
      />
      <AnimatePresence initial={false}>
        {hasChildren && isOpen && (
          <motion.div
            initial={{ height: 0, opacity: 0 }}
            animate={{ height: "auto", opacity: 1 }}
            exit={{ height: 0, opacity: 0 }}
            transition={{ duration: 0.18, ease: "easeOut" }}
            className="overflow-hidden"
          >
            <PageTree pages={pages} parent={page.id} depth={depth + 1} onCreateChild={onCreateChild} />
          </motion.div>
        )}
      </AnimatePresence>
    </div>
  );
}
