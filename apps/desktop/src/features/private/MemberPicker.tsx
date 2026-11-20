import { Check, Plus } from "lucide-react";
import { api, type Member } from "../../lib/api";
import { useLive } from "../../lib/useLive";
import { useWorkspace } from "../../lib/workspace";
import { Button } from "../../ui";
import { SearchableMemberList } from "../members/MemberList";

/** Current members other than you, to pick some of. */
export function useOtherMembers(): Member[] | undefined {
  const { session } = useWorkspace();
  const members = useLive(api.members, (change) => change.kind === "members");
  return members?.filter((member) => member.active && member.id !== session.me);
}

/** A searchable list where each member can be picked or unpicked. */
export function MemberPicker({
  label,
  members,
  chosen,
  onToggle,
}: {
  label: string;
  members: Member[];
  chosen: ReadonlySet<string>;
  onToggle: (member: Member) => void;
}) {
  return (
    <div className="max-h-[320px] overflow-y-auto">
      <SearchableMemberList
        label={label}
        members={members}
        actions={(member) => {
          const isChosen = chosen.has(member.id);
          return (
            <Button
              size="sm"
              variant={isChosen ? "secondary" : "ghost"}
              aria-pressed={isChosen}
              aria-label={`${isChosen ? "Remove" : "Add"} ${member.name}`}
              icon={isChosen ? <Check size={13} /> : <Plus size={13} />}
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
