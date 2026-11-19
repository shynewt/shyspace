import { ChevronRight, FilePlus2, Link2, Trash2 } from "lucide-react";
import { motion } from "motion/react";
import { useEffect, useState } from "react";
import { api, type Page, pageLink } from "../../lib/api";
import { reportError, showToast } from "../../lib/toast";
import { useWorkspace } from "../../lib/workspace";
import { Button, ConfirmDialog } from "../../ui";
import { Editor } from "./Editor";

function ancestorsOf(page: Page | undefined, pages: Page[]): Page[] {
  const ancestors: Page[] = [];
  for (let parent = page?.parent; parent; ) {
    const ancestor = pages.find((candidate) => candidate.id === parent);
    if (!ancestor) break;
    ancestors.unshift(ancestor);
    parent = ancestor.parent;
  }
  return ancestors;
}

export function PageView({ pageId }: { pageId: string }) {
  const { pages, navigate } = useWorkspace();
  const page = pages.find((candidate) => candidate.id === pageId);
  const [isConfirmingDelete, setConfirmingDelete] = useState(false);

  const copyLink = () =>
    navigator.clipboard
      .writeText(pageLink(pageId))
      .then(() => showToast("Link copied. Paste it in any channel to share this page."), reportError);

  const createSubpage = async () => {
    try {
      navigate({ kind: "page", id: await api.createPage("", pageId) });
    } catch (error) {
      reportError(error);
    }
  };

  return (
    <div className="flex h-full flex-col">
      <header className="flex h-13 shrink-0 items-center gap-1 border-b border-line px-5 text-[13px] text-muted">
        <nav aria-label="Breadcrumb" className="flex min-w-0 items-center gap-1">
          {ancestorsOf(page, pages).map((ancestor) => (
            <span key={ancestor.id} className="flex items-center gap-1">
              <button
                type="button"
                onClick={() => navigate({ kind: "page", id: ancestor.id })}
                className="rounded px-1 hover:bg-surface-2 hover:text-ink"
              >
                {ancestor.title || "Untitled"}
              </button>
              <ChevronRight size={13} className="text-faint" />
            </span>
          ))}
          <span className="truncate px-1 text-ink">{page?.title || "Untitled"}</span>
        </nav>
        <div className="ml-auto flex items-center gap-1">
          <Button variant="ghost" size="sm" icon={<FilePlus2 size={14} />} onClick={createSubpage}>
            Sub-page
          </Button>
          <Button variant="ghost" size="sm" icon={<Link2 size={14} />} onClick={copyLink}>
            Copy link
          </Button>
          {page?.can_delete && (
            <Button variant="ghost" size="sm" icon={<Trash2 size={14} />} onClick={() => setConfirmingDelete(true)}>
              Delete
            </Button>
          )}
        </div>
      </header>
      <ConfirmDialog
        open={isConfirmingDelete}
        onOpenChange={setConfirmingDelete}
        title={`Delete "${page?.title || "Untitled"}"?`}
        description="The page disappears for everyone. Its sub-pages stay and move to the top level. This can't be undone."
        confirmLabel="Delete page"
        onConfirm={() => api.deletePage(pageId)}
      />
      <div className="flex-1 overflow-y-auto">
        <motion.article
          className="mx-auto max-w-[760px] px-12 pt-14"
          initial={{ opacity: 0, y: 8 }}
          animate={{ opacity: 1, y: 0 }}
          transition={{ delay: 0.04 }}
        >
          <TitleInput pageId={pageId} title={page?.title ?? ""} />
          <div className="mt-5">
            <Editor key={pageId} pageId={pageId} />
          </div>
        </motion.article>
      </div>
    </div>
  );
}

/** Saves on blur or Enter, and follows remote renames while not editing. */
function TitleInput({ pageId, title }: { pageId: string; title: string }) {
  const [draft, setDraft] = useState(title);
  const [isEditing, setEditing] = useState(false);

  useEffect(() => {
    if (!isEditing) setDraft(title);
  }, [title, isEditing]);

  const save = () => {
    setEditing(false);
    if (draft.trim() !== title) api.renamePage(pageId, draft).catch(reportError);
  };

  return (
    <input
      aria-label="Page title"
      value={draft}
      onFocus={() => setEditing(true)}
      onChange={(event) => setDraft(event.target.value)}
      onBlur={save}
      onKeyDown={(event) => {
        if (event.key !== "Enter") return;
        event.preventDefault();
        event.currentTarget.closest("article")?.querySelector<HTMLElement>(".cm-content")?.focus();
      }}
      placeholder="Untitled"
      className="w-full bg-transparent text-[38px] leading-tight font-bold tracking-tight outline-none placeholder:text-surface-3"
    />
  );
}
