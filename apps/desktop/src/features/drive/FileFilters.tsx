import { MessagesSquare, Search, Users } from "lucide-react";
import { useMemo } from "react";
import type { Channel, FileInfo, Member } from "../../lib/api";
import { channelTitle } from "../../lib/channels";
import { type FileKind, fileKindOf } from "../../lib/fileKinds";
import { displayName } from "../../lib/people";
import { useWorkspace } from "../../lib/workspace";
import { Input, SegmentedControl, Select, type SelectOption } from "../../ui";
import { MemberAvatar } from "../members/MemberAvatar";
import { ChannelGlyph } from "../private/ChannelGlyph";

type Kind = "all" | "images" | "documents" | "other";

const KINDS: Array<{ value: Kind; label: string }> = [
  { value: "all", label: "All" },
  { value: "images", label: "Images" },
  { value: "documents", label: "Documents" },
  { value: "other", label: "Other" },
];

/** The same kinds that pick a file's icon, so a filter never disagrees with what a row shows. */
const KIND_FILTER: Record<FileKind, Exclude<Kind, "all">> = {
  image: "images",
  document: "documents",
  spreadsheet: "documents",
  slides: "documents",
  code: "documents",
  data: "documents",
  archive: "other",
  audio: "other",
  video: "other",
  other: "other",
};

/** "Anyone" and "anywhere": no member or channel id looks like this. */
const ANY = "any";

export type Filters = { query: string; kind: Kind; sharer: string; place: string };

export const NO_FILTERS: Filters = { query: "", kind: "all", sharer: ANY, place: ANY };

export function matchesFilters(file: FileInfo, filters: Filters): boolean {
  return (
    file.name.toLowerCase().includes(filters.query.trim().toLowerCase()) &&
    (filters.kind === "all" || KIND_FILTER[fileKindOf(file.name, file.media_type)] === filters.kind) &&
    (filters.sharer === ANY || file.author === filters.sharer) &&
    (filters.place === ANY || file.place === filters.place)
  );
}

/**
 * Search by name above one row of filters: kind, who shared it and where.
 * The two lists offer only people and places that have files, most recent first.
 */
export function FileFilters({
  files,
  filters,
  onChange,
}: {
  files: FileInfo[];
  filters: Filters;
  onChange: (filters: Filters) => void;
}) {
  const { members, channels } = useWorkspace();
  const sharers = useMemo(() => sharerOptions(files, members), [files, members]);
  const places = useMemo(() => placeOptions(files, channels, members), [files, channels, members]);
  const change = (update: Partial<Filters>) => onChange({ ...filters, ...update });
  return (
    <div className="@container/filters flex flex-col gap-2">
      <div className="relative">
        <Search size={14} className="pointer-events-none absolute top-1/2 left-3 -translate-y-1/2 text-decorative" />
        <Input
          type="search"
          aria-label="Search files by name"
          placeholder="Search by name"
          value={filters.query}
          onChange={(event) => change({ query: event.target.value })}
          className="pl-8"
        />
      </div>
      {/* One row when it fits, the lists sharing what the kinds leave; in a narrow
          window the kinds take a full row and the lists share the next. */}
      <div className="flex flex-wrap items-center gap-2">
        <div className="w-full @filters-one-row/filters:w-auto">
          <SegmentedControl
            label="Kind of file"
            options={KINDS}
            value={filters.kind}
            onChange={(kind) => change({ kind })}
            width="full"
          />
        </div>
        <div className="flex min-w-0 flex-1 basis-96 gap-2">
          <Select
            aria-label="Shared by"
            value={filters.sharer}
            options={sharers}
            onChange={(sharer) => change({ sharer })}
            isActive={filters.sharer !== ANY}
            className="min-w-0 flex-1"
          />
          <Select
            aria-label="In"
            value={filters.place}
            options={places}
            onChange={(place) => change({ place })}
            isActive={filters.place !== ANY}
            className="min-w-0 flex-1"
          />
        </div>
      </div>
    </div>
  );
}

function sharerOptions(files: FileInfo[], members: Member[] | undefined): SelectOption<string>[] {
  const nameOf = (id: string) => displayName(members?.find((member) => member.id === id)?.name ?? "", id);
  return [
    { value: ANY, label: "Shared by anyone", icon: <Users size={14} /> },
    ...[...new Set(files.map((file) => file.author))].map((id) => ({
      value: id,
      label: nameOf(id),
      icon: <MemberAvatar id={id} name={nameOf(id)} size={16} />,
    })),
  ];
}

function placeOptions(files: FileInfo[], channels: Channel[], members: Member[] | undefined): SelectOption<string>[] {
  const withFiles = [...new Set(files.map((file) => file.place))].flatMap((id) => {
    const channel = channels.find((candidate) => candidate.id === id);
    return channel ? [channel] : [];
  });
  return [
    { value: ANY, label: "Anywhere", icon: <MessagesSquare size={14} /> },
    ...withFiles.map((channel) => {
      const title = channelTitle(channel, members);
      const [person] = channel.dm_members;
      return {
        value: channel.id,
        label: title,
        icon:
          channel.kind === "dm" && person ? (
            <MemberAvatar id={person} name={title} size={16} />
          ) : (
            <ChannelGlyph kind={channel.kind} size={14} className="text-decorative" />
          ),
      };
    }),
  ];
}
