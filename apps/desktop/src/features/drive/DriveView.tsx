import { HardDrive, MessageSquareText } from "lucide-react";
import { useMemo, useState } from "react";
import { api, type FileInfo } from "../../lib/api";
import { channelTitle } from "../../lib/channels";
import { formatSize, useFileImage } from "../../lib/files";
import { displayName } from "../../lib/people";
import { useLive } from "../../lib/useLive";
import { useWorkspace } from "../../lib/workspace";
import { EmptyState, IconButton, Input, RelativeTime, SectionLabel, SegmentedControl, Select } from "../../ui";
import { FileActions } from "../files/FileActions";
import { FileThumbnail } from "../files/FileThumbnail";
import { ImageViewer } from "../files/ImageViewer";
import { ChannelName } from "../private/ChannelName";
import { ViewHeader } from "../shell/ViewHeader";

type Kind = "all" | "images" | "documents" | "other";

const KINDS: Array<{ value: Kind; label: string }> = [
  { value: "all", label: "All" },
  { value: "images", label: "Images" },
  { value: "documents", label: "Documents" },
  { value: "other", label: "Other" },
];

function kindOf(file: FileInfo): Exclude<Kind, "all"> {
  if (file.media_type.startsWith("image/")) return "images";
  const isDocument =
    file.media_type.startsWith("text/") ||
    file.media_type === "application/pdf" ||
    file.media_type.includes("officedocument") ||
    file.media_type === "application/json";
  return isDocument ? "documents" : "other";
}

/** Every file shared where you can read, grouped by where it was shared. */
export function DriveView() {
  const files = useLive(api.files, (change) => change.kind === "messages" || change.kind === "channels");
  const { members, channels } = useWorkspace();
  const [query, setQuery] = useState("");
  const [kind, setKind] = useState<Kind>("all");
  const [person, setPerson] = useState("");

  const nameOf = (id: string) => displayName(members?.find((member) => member.id === id)?.name ?? "", id);
  const placeName = (place: string) => {
    const channel = channels.find((candidate) => candidate.id === place);
    return channel ? channelTitle(channel, members) : "a channel";
  };
  const sharers = useMemo(() => [...new Set((files ?? []).map((file) => file.author))], [files]);
  const shown = (files ?? []).filter(
    (file) =>
      file.name.toLowerCase().includes(query.trim().toLowerCase()) &&
      (kind === "all" || kindOf(file) === kind) &&
      (!person || file.author === person),
  );
  const places = [...new Set(shown.map((file) => file.place))].filter((place) =>
    channels.some((channel) => channel.id === place),
  );

  return (
    <div className="flex h-full flex-col">
      <ViewHeader>
        <HardDrive size={17} className="shrink-0 text-decorative" />
        <h1 className="truncate text-lg font-semibold">Files</h1>
      </ViewHeader>
      <div className="flex-1 overflow-y-auto px-5 py-4">
        <div className="mx-auto flex max-w-reading flex-col gap-4">
          {files && files.length === 0 ? (
            <EmptyState icon={<HardDrive size={22} />} title="No files yet">
              Files shared in channels and direct messages you can read show up here. Attach one from the message box.
            </EmptyState>
          ) : (
            <>
              <div className="flex flex-wrap items-center gap-2">
                <Input
                  type="search"
                  aria-label="Search files by name"
                  placeholder="Search by name"
                  value={query}
                  onChange={(event) => setQuery(event.target.value)}
                  className="w-56 flex-1"
                />
                <SegmentedControl label="Kind of file" options={KINDS} value={kind} onChange={setKind} />
                <Select
                  aria-label="Shared by"
                  value={person}
                  onChange={(event) => setPerson(event.target.value)}
                  className="w-40"
                >
                  <option value="">Anyone</option>
                  {sharers.map((id) => (
                    <option key={id} value={id}>
                      {nameOf(id)}
                    </option>
                  ))}
                </Select>
              </div>
              {files && shown.length === 0 && <p className="px-1 text-sm text-muted">No files match.</p>}
              {places.map((place) => (
                <section key={place} aria-label={`Files in ${placeName(place)}`}>
                  <SectionLabel>
                    <ChannelName channelId={place} />
                  </SectionLabel>
                  <ul className="flex flex-col gap-1">
                    {shown
                      .filter((file) => file.place === place)
                      .map((file) => (
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
  const { navigate } = useWorkspace();
  const image = useFileImage(file);
  const [isViewing, setViewing] = useState(false);
  const summary = (
    <>
      <FileThumbnail mediaType={file.media_type} image={image} size="sm" />
      <span className="min-w-0 flex-1">
        <span className="block truncate text-sm font-medium">{file.name}</span>
        <span className="block truncate text-xs text-faint">
          {formatSize(file.size)} · {sharedBy} · <RelativeTime ms={file.created_ms} />
        </span>
      </span>
    </>
  );
  return (
    <li className="flex items-center gap-3 rounded-lg px-2.5 py-1.5 hover:bg-surface-2">
      {image.status === "shown" ? (
        <>
          <button
            type="button"
            aria-label={`View ${file.name}`}
            onClick={() => setViewing(true)}
            className="flex min-w-0 flex-1 cursor-zoom-in items-center gap-3 rounded-md text-left"
          >
            {summary}
          </button>
          <ImageViewer file={file} url={image.url} open={isViewing} onOpenChange={setViewing} />
        </>
      ) : (
        <div className="flex min-w-0 flex-1 items-center gap-3">{summary}</div>
      )}
      {file.message && (
        <IconButton
          label={`Show ${file.name} in chat`}
          icon={<MessageSquareText size={15} />}
          onClick={() => file.message && navigate({ kind: "channel", id: file.place, focus: file.message })}
        />
      )}
      <FileActions file={file} />
    </li>
  );
}
