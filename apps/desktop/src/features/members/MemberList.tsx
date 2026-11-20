import { Search } from "lucide-react";
import { type ReactNode, useState } from "react";
import type { Member } from "../../lib/api";
import { displayName } from "../../lib/people";
import { useWorkspace } from "../../lib/workspace";
import { Avatar, Badge, Input } from "../../ui";

type ListProps = {
  /** The list's accessible name, like "Members". */
  label: string;
  members: Member[];
  /** Controls at the end of a member's row, like a Remove button. */
  actions?: (member: Member) => ReactNode;
};

/** Members with a search box above them that filters by name. */
export function SearchableMemberList({ label, members, actions }: ListProps) {
  const [query, setQuery] = useState("");
  const needle = query.trim().toLowerCase();
  const shown = needle
    ? members.filter((member) => displayName(member.name, member.id).toLowerCase().includes(needle))
    : members;
  return (
    <div className="flex flex-col gap-2">
      <div className="relative">
        <Search size={14} className="pointer-events-none absolute top-1/2 left-3 -translate-y-1/2 text-faint" />
        <Input
          type="search"
          value={query}
          onChange={(event) => setQuery(event.target.value)}
          placeholder="Search by name"
          aria-label={`Search ${label.toLowerCase()}`}
          className="pl-8"
        />
      </div>
      {shown.length > 0 ? (
        <MemberRows label={label} members={shown} actions={actions} />
      ) : (
        <p className="px-2 py-6 text-center text-[13px] text-muted">No one matches “{query.trim()}”.</p>
      )}
    </div>
  );
}

/** Members, each with their presence or status, role and whether they're an agent. */
export function MemberRows({ label, members, actions }: ListProps) {
  const { session } = useWorkspace();
  return (
    <ul aria-label={label} className="flex flex-col">
      {members.map((member) => (
        <MemberRow key={member.id} member={member} isMe={member.id === session.me} actions={actions?.(member)} />
      ))}
    </ul>
  );
}

const PRESENCE_LABELS: Record<Member["presence"], string> = {
  online: "Online",
  away: "Away",
  unreachable: "Not reachable right now",
};

function MemberRow({ member, isMe, actions }: { member: Member; isMe: boolean; actions: ReactNode }) {
  const name = displayName(member.name, member.id);
  return (
    <li className="flex min-h-12 items-center gap-3 rounded-lg px-2 py-1.5">
      <Avatar
        id={member.id}
        name={name}
        isBot={member.is_bot}
        size={32}
        presence={member.active ? member.presence : undefined}
      />
      <div className="min-w-0 flex-1">
        <div className="flex items-center gap-1.5 text-[13.5px] font-medium">
          <span className="truncate">{name}</span>
          {isMe && <span className="shrink-0 text-[12px] font-normal text-muted">(you)</span>}
          {member.is_admin && <Badge>Admin</Badge>}
          {member.is_bot && <Badge tone="accent">Bot</Badge>}
        </div>
        <div className="truncate text-[12px] text-muted">
          <MemberStatusLine member={member} />
        </div>
      </div>
      {actions}
    </li>
  );
}

function MemberStatusLine({ member }: { member: Member }) {
  if (!member.active) return <span>Removed</span>;
  if (!member.status) return <span>{PRESENCE_LABELS[member.presence]}</span>;
  return (
    <span>
      {member.status.emoji} {member.status.text}
      {member.status.alerts !== "all" && (
        <span className="ml-1 text-warning">· {member.status.alerts === "nothing" ? "Busy" : "Mentions only"}</span>
      )}
    </span>
  );
}
