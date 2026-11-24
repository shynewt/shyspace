import { Bookmark, BookmarkCheck, ChevronRight, Ellipsis, FilePlus2, Link2, Trash2 } from "lucide-react";
import { useCallback, useRef, useState } from "react";
import { api, type Page, pageLink } from "../../lib/api";
import { useMembers } from "../../lib/members";
import { displayName } from "../../lib/people";
import { reportError, showToast } from "../../lib/toast";
import { useLive } from "../../lib/useLive";
import { useWorkspace } from "../../lib/workspace";
import { ConfirmDialog, cx, IconButton, Menu, type MenuItem } from "../../ui";
import { MemberAvatar } from "../members/MemberAvatar";
import { HeaderAction, ViewHeader } from "../shell/ViewHeader";
import { type CursorJump, Editor } from "./Editor";
import { PageTitle } from "./PageTitle";

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
  const { pages, navigate, session } = useWorkspace();
  const page = pages.find((candidate) => candidate.id === pageId);
  const [isConfirmingDelete, setConfirmingDelete] = useState(false);
  const title = useRef<{ focus: () => void }>(null);
  const loadPresence = useCallback(() => api.pagePresence(pageId), [pageId]);
  const presence = useLive(
    loadPresence,
    (change) => (change.kind === "page_presence" && change.page === pageId) || change.kind === "members",
  );
  const [jumpTo, setJumpTo] = useState<CursorJump | null>(null);

  const copyLink = () =>
    navigator.clipboard
      .writeText(pageLink(session.workspace_id, pageId))
      .then(() => showToast("Link copied. Paste it in any channel to share this page."), reportError);

  const createSubpage = async () => {
    try {
      navigate({ kind: "page", id: await api.createPage("", pageId) });
    } catch (error) {
      reportError(error);
    }
  };

  const actions: MenuItem[] = [{ label: "New sub-page", icon: <FilePlus2 size={14} />, onSelect: createSubpage }];
  if (page?.can_delete) {
    actions.push({
      label: "Delete page",
      icon: <Trash2 size={14} />,
      onSelect: () => setConfirmingDelete(true),
      isDestructive: true,
    });
  }

  return (
    <div className="flex h-full flex-col">
      <ViewHeader className="text-sm text-muted">
        <nav aria-label="Breadcrumb" className="min-w-0 flex-1">
          <ol className="flex min-w-0 items-center gap-0.5">
            {ancestorsOf(page, pages).map((ancestor, index, ancestors) => (
              <li
                key={ancestor.id}
                className={cx(
                  "flex min-w-0 shrink items-center gap-0.5",
                  // A narrow header keeps only the parent.
                  index < ancestors.length - 1 && "@max-header-labels/header:hidden",
                )}
              >
                <button
                  type="button"
                  onClick={() => navigate({ kind: "page", id: ancestor.id })}
                  className="max-w-45 truncate rounded px-1 hover:bg-surface-2 hover:text-ink"
                >
                  {ancestor.title || "Untitled"}
                </button>
                <ChevronRight size={13} aria-hidden className="shrink-0 text-decorative" />
              </li>
            ))}
            <li className="flex min-w-30 items-center">
              <button
                type="button"
                aria-current="page"
                onClick={() => title.current?.focus()}
                className="truncate rounded px-1 text-ink hover:bg-surface-2"
              >
                {page?.title || "Untitled"}
              </button>
            </li>
          </ol>
        </nav>
        <div className="flex shrink-0 items-center gap-1">
          <PageViewers viewers={presence?.viewers ?? []} onGoTo={(member) => setJumpTo({ member, at: Date.now() })} />
          <HeaderAction icon={<Link2 size={14} />} label="Copy link" onClick={copyLink} />
          {page && (
            <IconButton
              label={page.is_saved ? "Saved for later" : "Save for later"}
              aria-pressed={page.is_saved}
              isActive={page.is_saved}
              icon={page.is_saved ? <BookmarkCheck size={15} /> : <Bookmark size={15} />}
              onClick={() =>
                (page.is_saved ? api.removeFromLater(page.id) : api.saveForLater(page.id, null)).catch(reportError)
              }
            />
          )}
          <Menu trigger={<IconButton label="More page actions" icon={<Ellipsis size={15} />} />} items={actions} />
        </div>
      </ViewHeader>
      <ConfirmDialog
        open={isConfirmingDelete}
        onOpenChange={setConfirmingDelete}
        title={`Delete "${page?.title || "Untitled"}"?`}
        description="The page disappears for everyone. Its sub-pages stay and move to the top level. This can't be undone."
        confirmLabel="Delete page"
        onConfirm={() => api.deletePage(pageId)}
      />
      <div className="flex-1 overflow-y-auto">
        <article className="mx-auto max-w-reading px-6 pt-8 @2xl/main:px-12 @2xl/main:pt-14">
          <PageTitle ref={title} pageId={pageId} title={page?.title ?? ""} />
          <div className="mt-5">
            <Editor key={pageId} pageId={pageId} presence={presence} jumpTo={jumpTo} />
          </div>
        </article>
      </div>
    </div>
  );
}

/** Who else is on the page; each one goes to their cursor. */
function PageViewers({ viewers, onGoTo }: { viewers: string[]; onGoTo: (member: string) => void }) {
  const members = useMembers();
  if (viewers.length === 0) return null;
  return (
    <ul aria-label="On this page" className="mr-1 flex items-center -space-x-1.5">
      {viewers.map((member) => {
        const name = displayName(members?.find((candidate) => candidate.id === member)?.name ?? "", member);
        return (
          <li key={member}>
            <button
              type="button"
              aria-label={`Go to ${name}'s cursor`}
              title={name}
              onClick={() => onGoTo(member)}
              className="rounded-full ring-2 ring-surface"
            >
              <MemberAvatar id={member} name={name} size={22} />
            </button>
          </li>
        );
      })}
    </ul>
  );
}
