import { ChevronRight, Ellipsis, FileText, Link2, Plus, Trash2 } from "lucide-react";
import { AnimatePresence, motion } from "motion/react";
import { useMemo, useState } from "react";
import { api, type Page, pageLink } from "../../lib/api";
import { startPageDrag } from "../../lib/pageDrag";
import { reportError, showToast } from "../../lib/toast";
import { ConfirmDialog, cx, IconButton, Menu, type MenuItem, springs } from "../../ui";
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
  const [isMenuOpen, setMenuOpen] = useState(false);
  const [isConfirmingDelete, setConfirmingDelete] = useState(false);
  const copyLink = () =>
    navigator.clipboard
      .writeText(pageLink(page.id))
      .then(() => showToast("Link copied. Paste it in any channel to share this page."), reportError);
  const items: MenuItem[] = [
    { label: "New sub-page", icon: <Plus size={14} />, onSelect: () => onCreateChild(page.id) },
    { label: "Copy link", icon: <Link2 size={14} />, onSelect: copyLink },
  ];
  if (page.can_delete) {
    items.push({
      label: "Delete page",
      icon: <Trash2 size={14} />,
      onSelect: () => setConfirmingDelete(true),
      isDestructive: true,
    });
  }
  const hasChildren = pages.some((candidate) => candidate.parent === page.id);
  const title = page.title || "Untitled";
  return (
    <div>
      <NavItem
        view={{ kind: "page", id: page.id }}
        depth={depth}
        label={title}
        icon={<FileText size={15} />}
        onContextMenu={() => setMenuOpen(true)}
        onDragStart={(event) => startPageDrag(event, page.id)}
        leading={
          // Pages without sub-pages keep the toggle's space, so nesting lines up.
          hasChildren ? (
            <button
              type="button"
              aria-expanded={isOpen}
              aria-label={`${isOpen ? "Collapse" : "Expand"} ${title}`}
              onClick={() => setOpen(!isOpen)}
              className="relative -mr-1 grid h-6 w-5 shrink-0 place-items-center rounded text-decorative hover:text-ink"
            >
              <motion.span animate={{ rotate: isOpen ? 90 : 0 }} transition={springs.snappy} className="grid">
                <ChevronRight size={14} />
              </motion.span>
            </button>
          ) : (
            <span aria-hidden className="-mr-1 w-5 shrink-0" />
          )
        }
        trailing={
          // Beside the name rather than over it: it takes room only while shown, and the name truncates sooner.
          <span
            className={cx(
              "flex shrink-0 overflow-hidden",
              isMenuOpen
                ? "w-auto pl-0.5"
                : "w-0 opacity-0 group-hover:w-auto group-hover:pl-0.5 group-hover:opacity-100 keyboard-focus-within:w-auto keyboard-focus-within:pl-0.5 keyboard-focus-within:opacity-100",
            )}
          >
            <IconButton
              label={`New sub-page in ${title}`}
              size="sm"
              icon={<Plus size={13} />}
              onClick={() => onCreateChild(page.id)}
            />
            <Menu
              trigger={<IconButton label={`Actions for ${title}`} size="sm" icon={<Ellipsis size={13} />} />}
              items={items}
              open={isMenuOpen}
              onOpenChange={setMenuOpen}
            />
          </span>
        }
      />
      <ConfirmDialog
        open={isConfirmingDelete}
        onOpenChange={setConfirmingDelete}
        title={`Delete "${title}"?`}
        description="The page disappears for everyone. Its sub-pages stay and move to the top level. This can't be undone."
        confirmLabel="Delete page"
        onConfirm={() => api.deletePage(page.id)}
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
