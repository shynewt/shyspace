import { HardDrive, Search, Users } from "lucide-react";
import { useCallback, useMemo, useRef, useState } from "react";
import { api, type FileInfo } from "../../lib/api";
import { formatSize, useFileImage } from "../../lib/files";
import { displayName, groupByDay } from "../../lib/people";
import { useContextMenu } from "../../lib/useContextMenu";
import { useLive } from "../../lib/useLive";
import { useWorkspace } from "../../lib/workspace";
import {
  Button,
  cx,
  DayDivider,
  EmptyState,
  Input,
  ListRow,
  RelativeTime,
  SegmentedControl,
  Select,
  type SelectOption,
} from "../../ui";
import { FileActions } from "../files/FileActions";
import { FileThumbnail } from "../files/FileThumbnail";
import { FilePreview } from "../files/FileViewer";
import { MemberAvatar } from "../members/MemberAvatar";
import { ChannelName } from "../private/ChannelName";
import { ViewHeader } from "../shell/ViewHeader";

type Kind = "all" | "images" | "documents" | "other";

const KINDS: Array<{ value: Kind; label: string }> = [
  { value: "all", label: "All" },
  { value: "images", label: "Images" },
  { value: "documents", label: "Documents" },
  { value: "other", label: "Other" },
];

const ANYONE = "anyone";

function kindOf(file: FileInfo): Exclude<Kind, "all"> {
  if (file.media_type.startsWith("image/")) return "images";
  const isDocument =
    file.media_type.startsWith("text/") ||
    file.media_type === "application/pdf" ||
    file.media_type.includes("officedocument") ||
    file.media_type === "application/json";
  return isDocument ? "documents" : "other";
}

/** Every file shared where you can read, newest first, grouped by day. */
export function DriveView() {
  // The core lists files newest first, which the day groups rely on.
  const files = useLive(api.files, (change) => change.kind === "messages" || change.kind === "channels");
  const { members, channels } = useWorkspace();
  const [query, setQuery] = useState("");
  const [kind, setKind] = useState<Kind>("all");
  const [sharer, setSharer] = useState(ANYONE);

  const nameOf = useCallback(
    (id: string) => displayName(members?.find((member) => member.id === id)?.name ?? "", id),
    [members],
  );
  const sharerOptions = useMemo((): SelectOption<string>[] => {
    const ids = [...new Set((files ?? []).map((file) => file.author))];
    return [
      { value: ANYONE, label: "Shared by anyone", icon: <Users size={14} /> },
      ...ids.map((id) => ({
        value: id,
        label: nameOf(id),
        icon: <MemberAvatar id={id} name={nameOf(id)} size={16} />,
      })),
    ];
  }, [files, nameOf]);
  const shown = (files ?? []).filter(
    (file) =>
      channels.some((channel) => channel.id === file.place) &&
      file.name.toLowerCase().includes(query.trim().toLowerCase()) &&
      (kind === "all" || kindOf(file) === kind) &&
      (sharer === ANYONE || file.author === sharer),
  );
  const days = groupByDay(shown, (file) => file.created_ms);
  const clearFilters = () => {
    setQuery("");
    setKind("all");
    setSharer(ANYONE);
  };

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
              <div className="flex flex-wrap items-center gap-2">
                <div className="relative min-w-40 flex-1">
                  <Search
                    size={14}
                    className="pointer-events-none absolute top-1/2 left-3 -translate-y-1/2 text-decorative"
                  />
                  <Input
                    type="search"
                    aria-label="Search files by name"
                    placeholder="Search by name"
                    value={query}
                    onChange={(event) => setQuery(event.target.value)}
                    className="pl-8"
                  />
                </div>
                <SegmentedControl label="Kind of file" options={KINDS} value={kind} onChange={setKind} />
                <Select
                  aria-label="Shared by"
                  value={sharer}
                  options={sharerOptions}
                  onChange={setSharer}
                  className="w-48"
                />
              </div>
              {files && shown.length === 0 && (
                <div className="flex flex-col items-center gap-2 py-8 text-center">
                  <p className="text-sm text-muted">No files match.</p>
                  <Button variant="ghost" size="sm" onClick={clearFilters}>
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
            <ChannelName channelId={file.place} className="min-w-0" />
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
            <FileActions file={file} hasShowInChat isMenuOpen={isMenuOpen} onMenuOpenChange={setMenuOpen} />
          </span>
        }
      >
        {file.name}
      </ListRow>
      <FilePreview file={file} image={image} open={isViewing} onOpenChange={setViewing} hasShowInChat />
    </li>
  );
}
