import { Check, Plus } from "lucide-react";
import { api, type Member } from "../../lib/api";
import { distinctName } from "../../lib/people";
import { useLive } from "../../lib/useLive";
import { useWorkspace } from "../../lib/workspace";
import { Button, Spinner } from "../../ui";
import { SearchableMemberList } from "../members/MemberList";

/** Current members other than you, to pick some of. */
export function useOtherMembers(): Member[] | undefined {
  const { session } = useWorkspace();
  const members = useLive(api.members, (change) => change.kind === "members");
  return members?.filter((member) => member.active && member.id !== session.me);
}

/** A searchable list where each member can be picked. Rows stay put once picked, so focus never jumps. */
export function MemberPicker({
  label,
  members,
  chosen,
  busy = new Set(),
  onToggle,
}: {
  label: string;
  members: Member[] | undefined;
  chosen: ReadonlySet<string>;
  /** Picks still being saved. */
  busy?: ReadonlySet<string>;
  onToggle: (member: Member) => void;
}) {
  if (!members) {
    return (
      <div className="flex justify-center py-8 text-decorative">
        <Spinner size={16} />
      </div>
    );
  }
  if (members.length === 0) {
    return <p className="px-2 py-6 text-center text-sm text-muted">No one else is in this workspace yet.</p>;
  }
  return (
    <div className="max-h-[320px] overflow-y-auto">
      <SearchableMemberList
        label={label}
        members={members}
        actions={(member) => {
          const isChosen = chosen.has(member.id);
          const name = distinctName(member, members);
          return (
            <Button
              size="sm"
              variant={isChosen ? "secondary" : "ghost"}
              aria-pressed={isChosen}
              aria-label={`${isChosen ? "Remove" : "Add"} ${name}`}
              icon={isChosen ? <Check size={13} /> : <Plus size={13} />}
              isLoading={busy.has(member.id)}
              disabled={busy.has(member.id)}
              onClick={() => onToggle(member)}
            >
              {isChosen ? "Added" : "Add"}
            </Button>
          );
        }}
      />
    </div>
  );
}

/** Toggles `member` in a set of member ids. */
export function toggled(chosen: ReadonlySet<string>, member: Member): Set<string> {
  const next = new Set(chosen);
  if (!next.delete(member.id)) next.add(member.id);
  return next;
}
