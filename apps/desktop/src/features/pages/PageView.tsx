import { useEffect, useState } from "react";
import { motion } from "motion/react";
import { ChevronRight, FilePlus2, Link2 } from "lucide-react";
import { api, pageLink } from "../lib/api";
import { reportError, showToast } from "../lib/toast";
import { useWorkspace } from "../lib/workspace";
import { Editor } from "./Editor";

export function PageView({ pageId }: { pageId: string }) {
  const { pages, navigate } = useWorkspace();
  const page = pages.find((candidate) => candidate.id === pageId);
  const ancestors = [];
  for (let parent = page?.parent; parent; ) {
    const ancestor = pages.find((candidate) => candidate.id === parent);
    if (!ancestor) break;
    ancestors.unshift(ancestor);
    parent = ancestor.parent;
  }

  const copyLink = async () => {
    await navigator.clipboard.writeText(pageLink(pageId));
    showToast("Link copied. Paste it in any channel to share this page.");
  };

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
        {ancestors.map((ancestor) => (
          <span key={ancestor.id} className="flex items-center gap-1">
            <button onClick={() => navigate({ kind: "page", id: ancestor.id })} className="rounded px-1 hover:bg-surface-2 hover:text-ink">
              {ancestor.title || "Untitled"}
            </button>
            <ChevronRight size={13} className="text-faint" />
          </span>
        ))}
        <span className="truncate px-1 text-ink">{page?.title || "Untitled"}</span>
        <div className="ml-auto flex items-center gap-1">
          <HeaderButton onClick={createSubpage} label="Sub-page">
            <FilePlus2 size={14} />
          </HeaderButton>
          <HeaderButton onClick={copyLink} label="Copy link">
            <Link2 size={14} />
          </HeaderButton>
        </div>
      </header>
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
      value={draft}
      onFocus={() => setEditing(true)}
      onChange={(event) => setDraft(event.target.value)}
      onBlur={save}
      onKeyDown={(event) => {
        if (event.key === "Enter") {
          event.preventDefault();
          (event.currentTarget.closest("article")?.querySelector(".cm-content") as HTMLElement | null)?.focus();
        }
      }}
      placeholder="Untitled"
      className="w-full bg-transparent text-[38px] leading-tight font-bold tracking-tight outline-none placeholder:text-surface-3"
    />
  );
}

function HeaderButton({ onClick, label, children }: { onClick: () => void; label: string; children: React.ReactNode }) {
  return (
    <motion.button
      whileTap={{ scale: 0.94 }}
      onClick={onClick}
      className="flex h-7 items-center gap-1.5 rounded-md px-2 text-[12.5px] text-muted transition hover:bg-surface-2 hover:text-ink"
    >
      {children}
      {label}
    </motion.button>
  );
}
