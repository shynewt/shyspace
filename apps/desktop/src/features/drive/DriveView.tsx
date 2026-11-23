import { FileText, HardDrive } from "lucide-react";
import { useCallback, useMemo, useRef, useState } from "react";
import { api, type FileInfo } from "../../lib/api";
import { formatSize, useFileImage } from "../../lib/files";
import { displayName, groupByDay } from "../../lib/people";
import { useContextMenu } from "../../lib/useContextMenu";
import { useLive } from "../../lib/useLive";
import { useWorkspace } from "../../lib/workspace";
import { Button, cx, DayDivider, EmptyState, ListRow, RelativeTime } from "../../ui";
import { FileActions } from "../files/FileActions";
import { FileThumbnail } from "../files/FileThumbnail";
import { FilePreview } from "../files/FileViewer";
import { ChannelName } from "../private/ChannelName";
import { ViewHeader } from "../shell/ViewHeader";
import { FileFilters, matchesFilters, NO_FILTERS } from "./FileFilters";

/** Every file shared where you can read, newest first, grouped by day. */
export function DriveView() {
  const files = useLive(
    api.files,
    (change) =>
      change.kind === "messages" ||
      change.kind === "channels" ||
      change.kind === "page_files" ||
      change.kind === "pages",
  );
  const { members, channels, pages } = useWorkspace();
  const [filters, setFilters] = useState(NO_FILTERS);

  const nameOf = useCallback(
    (id: string) => displayName(members?.find((member) => member.id === id)?.name ?? "", id),
    [members],
  );
  const readable = useMemo(
    () =>
      (files ?? []).filter((file) =>
        file.message === null
          ? pages.some((page) => page.id === file.place)
          : channels.some((channel) => channel.id === file.place),
      ),
    [files, channels, pages],
  );
  // The core lists files in the order they were written down; the view promises newest first.
  const shown = readable
    .filter((file) => matchesFilters(file, filters))
    .sort((left, right) => right.created_ms - left.created_ms);
  const days = groupByDay(shown, (file) => file.created_ms);

  return (
    <div className="flex h-full flex-col">
      <ViewHeader>
        <HardDrive size={17} className="shrink-0 text-decorative" />
        <h1 className="truncate text-lg font-semibold">Files</h1>
      </ViewHeader>
      <div className="flex-1 overflow-y-auto px-5 py-4">
        <div className="mx-auto flex max-w-reading flex-col gap-2">
          {files && files.length === 0 ? (
            <EmptyState icon={<HardDrive size={22} />} title="No files yet">
              Files shared in channels and direct messages you can read show up here. Attach one from the message box.
            </EmptyState>
          ) : (
            <>
              <FileFilters files={readable} filters={filters} onChange={setFilters} />
              {files && shown.length === 0 && (
                <div className="flex flex-col items-center gap-2 py-8 text-center">
                  <p className="text-sm text-muted">No files match.</p>
                  <Button variant="ghost" size="sm" onClick={() => setFilters(NO_FILTERS)}>
                    Clear filters
                  </Button>
                </div>
              )}
              {days.map((day) => (
                <section key={day.label} aria-label={day.label}>
                  <DayDivider label={day.label} />
                  <ul className="flex flex-col gap-1">
                    {day.items.map((file) => (
                      <DriveRow key={file.id} file={file} sharedBy={nameOf(file.author)} />
                    ))}
                  </ul>
                </section>
              ))}
            </>
          )}
        </div>
      </div>
    </div>
  );
}

/** Where a file was shared: its channel or DM, or its page by title. */
function FilePlace({ file }: { file: FileInfo }) {
  const { pages } = useWorkspace();
  if (file.message !== null) return <ChannelName channelId={file.place} className="min-w-0" />;
  const title = pages.find((page) => page.id === file.place)?.title || "Untitled";
  return (
    <span className="flex min-w-0 items-center gap-1">
      <FileText size={13} className="shrink-0 text-decorative" aria-hidden />
      <span className="truncate">{title}</span>
    </span>
  );
}

function DriveRow({ file, sharedBy }: { file: FileInfo; sharedBy: string }) {
  const image = useFileImage(file);
  const [isViewing, setViewing] = useState(false);
  const [isMenuOpen, setMenuOpen] = useState(false);
  const row = useRef<HTMLLIElement>(null);
  const openMenu = useCallback(() => setMenuOpen(true), []);
  useContextMenu(row, openMenu);
  return (
    <li ref={row}>
      <ListRow
        onClick={() => setViewing(true)}
        isHighlighted={isMenuOpen}
        leadingAlignment="center"
        leading={<FileThumbnail name={file.name} mediaType={file.media_type} image={image} />}
        meta={
          <>
            <span className="min-w-0 truncate font-semibold text-ink">{sharedBy}</span>
            <span className="shrink-0">in</span>
            <FilePlace file={file} />
            <span className="ml-auto shrink-0 pl-2 text-faint">
              {formatSize(file.size)} · <RelativeTime ms={file.created_ms} />
            </span>
          </>
        }
        trailing={
          <span
            className={cx(
              "flex shrink-0 opacity-0 transition-opacity group-hover:opacity-100 keyboard-focus-within:opacity-100",
              isMenuOpen && "opacity-100",
            )}
          >
            <FileActions file={file} hasShowInPlace isMenuOpen={isMenuOpen} onMenuOpenChange={setMenuOpen} />
          </span>
        }
      >
        {file.name}
      </ListRow>
      <FilePreview file={file} image={image} open={isViewing} onOpenChange={setViewing} hasShowInPlace />
    </li>
  );
}
